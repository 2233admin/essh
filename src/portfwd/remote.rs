//! Remote (`-R`) port forwards. SPEC §6.4.
//!
//! The server listens; connections it accepts arrive back over SSH as
//! `forwarded-tcpip` channels, and essh proxies each one to a local address.
//! Two things stood in the way, and only one of them was real.
//!
//! ## The `&mut` that is not
//!
//! SPEC §6.4 records `tcpip_forward` needing `&mut Handle` while sessions
//! hold an `Arc`, and proposes moving to `Arc<Mutex<Handle>>`. Reading russh
//! 0.55 makes the shape of that clearer: `tcpip_forward` takes `&mut self`
//! and its immediate neighbour `cancel_tcpip_forward` takes `&self`, and both
//! bodies do exactly one thing — send a message on an `mpsc` and await a
//! `oneshot`. The `&mut` is incidental rather than load-bearing.
//!
//! We cannot fix that from here, so the question is what to pay instead.
//! `Arc<Mutex<Handle>>` is what the spec suggested, and it means every
//! existing caller — the metrics sweep, the divergence collector, the file
//! browser, the shell — takes a lock on the one object all of them share.
//! Held across a sweep, that blocks the shell; held narrowly, it means
//! changing eight helper signatures that currently take `&Handle`.
//!
//! So: a second connection, opened lazily the first time a session asks for a
//! remote forward, and used for nothing else. It costs one handshake and one
//! extra line in the host's auth log, for a feature most sessions never touch.
//! That is a different trade from the one the sampler faced — there the
//! alternative was a handshake *per poll*, forever — and it leaves the
//! existing 31 call sites alone.
//!
//! ## Channels that were dropped
//!
//! `server_channel_open_forwarded_tcpip` accepted the incoming channel and
//! returned. The connection looked established to whoever made it on the
//! remote side and then went nowhere, which is a worse failure than a refused
//! connection: it hangs rather than erroring.
//!
//! Now the handler looks the channel up in a routing table, dials the local
//! target, and pumps both directions until either end closes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

/// Where one remote forward's traffic goes, once it reaches us.
#[derive(Clone, Debug)]
pub struct Route {
    pub target_host: String,
    pub target_port: u16,
    /// Connections proxied so far, for the forwards panel's count. Shared,
    /// because the proxy tasks that increment it outlive the call that
    /// registered the route.
    pub connections: Arc<AtomicU64>,
}

/// The remote forwards one connection has asked for.
///
/// Keyed by the address and port the *server* is listening on, which is what
/// arrives on an incoming channel. A server that binds `0.0.0.0` reports the
/// address it was asked for, so the key is what we requested rather than what
/// the kernel chose.
#[derive(Default)]
pub struct Router {
    routes: Mutex<HashMap<(String, u32), Route>>,
}

/// Two routes are the same route when they point at the same place. The
/// connection counter is shared state rather than identity, so it is not part
/// of the comparison — an `Arc<AtomicU64>` has no meaningful `==` anyway.
impl PartialEq for Route {
    fn eq(&self, other: &Self) -> bool {
        self.target_host == other.target_host && self.target_port == other.target_port
    }
}

impl Router {
    pub async fn add(&self, bind_address: &str, bind_port: u32, route: Route) {
        self.routes
            .lock()
            .await
            .insert((bind_address.to_string(), bind_port), route);
    }

    pub async fn remove(&self, bind_address: &str, bind_port: u32) -> Option<Route> {
        self.routes
            .lock()
            .await
            .remove(&(bind_address.to_string(), bind_port))
    }

    /// The route for an incoming channel.
    ///
    /// Falls back to matching on the port alone. OpenSSH reports the address
    /// it bound, which is not always the string we asked for: request
    /// `localhost` and a server with `GatewayPorts no` reports `localhost`,
    /// but one configured otherwise may report `0.0.0.0`. Refusing the
    /// connection over that mismatch would be pedantry with a hang attached.
    pub async fn lookup(&self, address: &str, port: u32) -> Option<Route> {
        let routes = self.routes.lock().await;
        routes
            .get(&(address.to_string(), port))
            .or_else(|| routes.iter().find(|((_, p), _)| *p == port).map(|(_, r)| r))
            .cloned()
    }

    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.routes.lock().await.len()
    }
}

/// Proxy one accepted channel to its local target until either end closes.
///
/// Errors are not reported anywhere a user sees: this runs per connection,
/// inside a handler callback, and a target that refuses is the remote side's
/// problem to notice. What matters is that the channel is closed rather than
/// left open, so the far end gets a closed socket instead of a hang.
pub async fn proxy(
    mut channel: russh::Channel<russh::client::Msg>,
    route: Route,
) -> Result<(), std::io::Error> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("{}:{}", route.target_host, route.target_port);
    let mut local = match tokio::net::TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            // Close rather than drop. A dropped channel leaves whoever
            // connected on the remote side waiting on a socket that will
            // never answer, which is the failure this module replaced.
            let _ = channel.eof().await;
            let _ = channel.close().await;
            return Err(e);
        }
    };
    route.connections.fetch_add(1, Ordering::Relaxed);

    let mut buf = vec![0u8; 32 * 1024];
    loop {
        tokio::select! {
            // From the remote, to the local target.
            msg = channel.wait() => match msg {
                Some(russh::ChannelMsg::Data { data }) => {
                    if local.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            // From the local target, back to the remote.
            read = local.read(&mut buf) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if channel.data(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            },
        }
    }

    let _ = local.shutdown().await;
    let _ = channel.eof().await;
    let _ = channel.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(port: u16) -> Route {
        Route {
            target_host: "127.0.0.1".into(),
            target_port: port,
            connections: Arc::new(AtomicU64::new(0)),
        }
    }

    #[tokio::test]
    async fn a_route_is_found_by_the_address_it_was_registered_with() {
        let r = Router::default();
        r.add("localhost", 8080, route(80)).await;
        assert_eq!(r.lookup("localhost", 8080).await, Some(route(80)));
    }

    /// OpenSSH reports the address it bound, which is not always the string
    /// we asked for. Refusing the connection over that mismatch would be
    /// pedantry with a hang attached.
    #[tokio::test]
    async fn a_route_is_still_found_when_the_server_renames_the_address() {
        let r = Router::default();
        r.add("localhost", 8080, route(80)).await;
        assert_eq!(r.lookup("0.0.0.0", 8080).await, Some(route(80)));
        assert_eq!(r.lookup("", 8080).await, Some(route(80)));
    }

    /// A port nobody asked for is not routed anywhere. The handler refuses
    /// and closes, rather than proxying to whatever happened to be first in
    /// the map.
    #[tokio::test]
    async fn an_unrequested_port_has_no_route() {
        let r = Router::default();
        r.add("localhost", 8080, route(80)).await;
        assert_eq!(r.lookup("localhost", 9999).await, None);
    }

    #[tokio::test]
    async fn removing_a_forward_removes_its_route() {
        let r = Router::default();
        r.add("localhost", 8080, route(80)).await;
        assert_eq!(r.len().await, 1);
        assert_eq!(r.remove("localhost", 8080).await, Some(route(80)));
        assert_eq!(r.len().await, 0);
        assert_eq!(r.lookup("localhost", 8080).await, None);
    }

    /// The counter is shared, so the panel's connection count reflects
    /// proxies that outlive the call which registered the route.
    #[tokio::test]
    async fn the_connection_count_is_shared_with_the_registered_route() {
        let r = Router::default();
        let original = route(80);
        r.add("localhost", 8080, original.clone()).await;
        let found = r.lookup("localhost", 8080).await.expect("the route");
        found.connections.fetch_add(3, Ordering::Relaxed);
        assert_eq!(original.connections.load(Ordering::Relaxed), 3);
    }
}
