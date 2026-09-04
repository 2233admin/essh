pub mod remote;

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Port forwarding types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardDirection {
    Local,
    Remote,
}

impl fmt::Display for ForwardDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForwardDirection::Local => write!(f, "L"),
            ForwardDirection::Remote => write!(f, "R"),
        }
    }
}

/// What is actually true about a forward right now.
///
/// This replaced `active: bool`, which was set to `true` by the constructor
/// and never set to anything else. The manager listed two forwards as "Active"
/// while nothing was bound on either port: `start_local_forward` was never
/// called from anywhere, and its `TcpListener::bind` error path returned out
/// of a detached task where no one could see it. A forward is now told apart
/// from a wish for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardState {
    /// Requested; the bind has not returned yet.
    Starting,
    /// Bound. `connections` counts sockets accepted so far.
    Active { connections: u64 },
    /// Not bound, and why. The reason is the whole point: "Inactive" sent
    /// people to `ss -ltn` to find out what the program already knew.
    Failed { reason: String },
}

impl ForwardState {
    pub fn label(&self) -> &'static str {
        match self {
            ForwardState::Starting => "starting",
            ForwardState::Active { .. } => "active",
            ForwardState::Failed { .. } => "failed",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, ForwardState::Active { .. })
    }
}

#[derive(Clone, Debug)]
pub struct PortForward {
    pub id: String,
    pub direction: ForwardDirection,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub state: ForwardState,
    /// When the state last changed. SPEC §3.4's LAST CHECK column.
    ///
    /// The reference calls it a probe time, which would be true of a design
    /// that probes. SPEC §6.3 settled on the bind result *being* the state —
    /// we own the listener, so there is nothing to probe — and this is when
    /// that result was established. Reporting it as a probe would be a claim
    /// about a check nobody ran.
    pub state_since: Instant,
}

// ---------------------------------------------------------------------------
// PortForwardManager
// ---------------------------------------------------------------------------

pub struct PortForwardManager {
    pub forwards: Vec<PortForward>,
    pub selected: usize,
}

impl PortForwardManager {
    pub fn new() -> Self {
        Self {
            forwards: Vec::new(),
            selected: 0,
        }
    }

    pub fn add_local(
        &mut self,
        bind_host: &str,
        bind_port: u16,
        target_host: &str,
        target_port: u16,
    ) -> &PortForward {
        let fwd = PortForward {
            id: uuid::Uuid::new_v4().to_string(),
            direction: ForwardDirection::Local,
            bind_host: bind_host.to_string(),
            bind_port,
            target_host: target_host.to_string(),
            target_port,
            state: ForwardState::Starting,
            state_since: Instant::now(),
        };
        self.forwards.push(fwd);
        self.forwards.last().unwrap()
    }

    /// A remote (`-R`) forward: the *server* listens, and what it accepts
    /// comes back to us. SPEC §6.4.
    ///
    /// `bind_host` is the address the server is asked to bind, which most
    /// sshd configurations pin to loopback whatever you request
    /// (`GatewayPorts no`, the default).
    pub fn add_remote(
        &mut self,
        bind_host: &str,
        bind_port: u16,
        target_host: &str,
        target_port: u16,
    ) -> &PortForward {
        let fwd = PortForward {
            id: uuid::Uuid::new_v4().to_string(),
            direction: ForwardDirection::Remote,
            bind_host: bind_host.to_string(),
            bind_port,
            target_host: target_host.to_string(),
            target_port,
            state: ForwardState::Starting,
            state_since: Instant::now(),
        };
        self.forwards.push(fwd);
        self.forwards.last().unwrap()
    }

    /// Record what the bind actually did. The only way `Active` is ever set.
    pub fn set_state(&mut self, id: &str, state: ForwardState) -> bool {
        match self.forwards.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                // Only when the state actually changes. A connection count
                // ticking up is not a rebind, and stamping it as one would
                // make a forward bound an hour ago claim it was bound a
                // second ago every time somebody used it.
                let changed = std::mem::discriminant(&f.state) != std::mem::discriminant(&state);
                f.state = state;
                if changed {
                    f.state_since = Instant::now();
                }
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: &str) -> Option<&PortForward> {
        self.forwards.iter().find(|f| f.id == id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.forwards.len();
        self.forwards.retain(|f| f.id != id);
        let removed = self.forwards.len() < len_before;
        if removed && self.selected >= self.forwards.len() && !self.forwards.is_empty() {
            self.selected = self.forwards.len() - 1;
        }
        removed
    }

    /// How many of the configured forwards actually bound. SPEC §2.5.
    ///
    /// The handoff's terminal meta line reads `2 forwards`, which counts
    /// rules. Its own forwards panel, two screens later, shows one of those
    /// two failing to bind. Counting rules is the same defect as reporting
    /// `Active` with nothing listening, moved to a different line.
    ///
    /// `None` when there are no forwards at all: a session with none should
    /// say nothing rather than `0 of 0`.
    pub fn active_count(&self) -> Option<(usize, usize)> {
        if self.forwards.is_empty() {
            return None;
        }
        Some((
            self.forwards.iter().filter(|f| f.state.is_active()).count(),
            self.forwards.len(),
        ))
    }

    pub fn is_empty(&self) -> bool {
        self.forwards.is_empty()
    }

    pub fn select_next(&mut self) {
        if !self.forwards.is_empty() {
            self.selected = (self.selected + 1) % self.forwards.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.forwards.is_empty() {
            if self.selected == 0 {
                self.selected = self.forwards.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.forwards.get(self.selected).map(|f| f.id.as_str())
    }
}

/// Parse a forward spec like "L:8080:localhost:80" or "R:3306:localhost:3306".
/// Returns (direction, bind_port, target_host, target_port) on success.
pub fn parse_forward_spec(spec: &str) -> Option<(ForwardDirection, u16, String, u16)> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() != 4 {
        return None;
    }
    let direction = match parts[0] {
        "L" | "l" => ForwardDirection::Local,
        "R" | "r" => ForwardDirection::Remote,
        _ => return None,
    };
    let bind_port: u16 = parts[1].parse().ok()?;
    let target_host = parts[2].to_string();
    let target_port: u16 = parts[3].parse().ok()?;
    Some((direction, bind_port, target_host, target_port))
}

// ---------------------------------------------------------------------------
// Actual forwarding logic
// ---------------------------------------------------------------------------

/// Start a local TCP port forward: binds a local listener and proxies each
/// accepted connection through the SSH session via `channel_open_direct_tcpip`.
///
/// Returns a `JoinHandle` that can be aborted to cancel the forward.
#[allow(dead_code)]
/// Bind the local port and start proxying it over the SSH connection.
///
/// The bind is awaited *here*, before the accept loop is spawned, and its
/// error is returned to the caller. Previously the whole function body was one
/// `tokio::spawn`, so a failed bind hit `Err(_) => return` inside a detached
/// task: the port was not listening, nothing reported it, and the UI — which
/// had already written `active: true` — said "Active". A caller that cannot
/// see the bind fail cannot tell the truth about it afterwards.
///
/// `connections` counts accepted sockets, so the view can say "1 open
/// connection" from evidence rather than from optimism.
pub async fn bind_forward(
    bind_host: &str,
    bind_port: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(format!("{}:{}", bind_host, bind_port)).await
}

pub async fn start_local_forward(
    ssh_handle: Arc<russh::client::Handle<crate::ssh::ClientHandler>>,
    forward: &PortForward,
    connections: Arc<AtomicU64>,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let target_host = forward.target_host.clone();
    let target_port = forward.target_port as u32;

    let listener = bind_forward(&forward.bind_host, forward.bind_port).await?;

    Ok(tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };
            connections.fetch_add(1, Ordering::Relaxed);

            let handle = Arc::clone(&ssh_handle);
            let host = target_host.clone();

            tokio::spawn(async move {
                let channel: russh::Channel<russh::client::Msg> = match handle
                    .channel_open_direct_tcpip(&host, target_port, "127.0.0.1", 0)
                    .await
                {
                    Ok(ch) => ch,
                    Err(_) => return,
                };

                let (mut reader, mut writer) = tokio::io::split(stream);
                let mut channel = channel;

                // Proxy data bidirectionally
                let mut buf = vec![0u8; 8192];
                loop {
                    tokio::select! {
                        result = tokio::io::AsyncReadExt::read(&mut reader, &mut buf) => {
                            match result {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if channel.data(&buf[..n]).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        msg = channel.wait() => {
                            match msg {
                                Some(russh::ChannelMsg::Data { data }) => {
                                    if tokio::io::AsyncWriteExt::write_all(&mut writer, &data).await.is_err() {
                                        break;
                                    }
                                }
                                Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => break,
                                _ => {}
                            }
                        }
                    }
                }
            });
        }
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_is_empty() {
        let mgr = PortForwardManager::new();
        assert!(mgr.is_empty());
        assert!(mgr.forwards.is_empty());
        assert_eq!(mgr.active_count(), None);
    }

    #[test]
    fn a_new_forward_is_not_active_until_something_binds() {
        // The defect, in one assertion. `add_local` used to set
        // `active: true`, so the manager reported a forward as working before
        // anything had touched a socket.
        let mut mgr = PortForwardManager::new();
        let fwd = mgr.add_local("127.0.0.1", 8080, "localhost", 80);
        assert_eq!(fwd.direction, ForwardDirection::Local);
        assert_eq!(fwd.bind_port, 8080);
        assert_eq!(fwd.target_port, 80);
        assert_eq!(fwd.state, ForwardState::Starting);
        assert!(!fwd.state.is_active());
        assert!(!mgr.is_empty());
        assert_eq!(mgr.forwards.len(), 1);
    }

    #[tokio::test]
    async fn a_bind_that_fails_says_why() {
        // Hold a port, then ask for it. The error has to reach the caller —
        // it used to `return` out of a detached task where nobody saw it.
        let held = bind_forward("127.0.0.1", 0)
            .await
            .expect("bind an ephemeral port");
        let port = held.local_addr().unwrap().port();

        let err = bind_forward("127.0.0.1", port)
            .await
            .expect_err("the port is already held");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[tokio::test]
    async fn a_bind_that_succeeds_is_listening() {
        let listener = bind_forward("127.0.0.1", 0).await.expect("bind");
        let addr = listener.local_addr().unwrap();
        // A real connection completes, which is the evidence "active" claims.
        let connected = tokio::net::TcpStream::connect(addr).await;
        assert!(connected.is_ok(), "nothing accepted on a bound port");
    }

    /// SPEC §2.5. The count is of forwards that bound, not rules that exist.
    ///
    /// The handoff's terminal meta line says `2 forwards` while its own
    /// forwards panel, two screens later, shows one of the two refusing to
    /// bind. Counting rules is the "Active with nothing listening" defect
    /// moved to a different line.
    #[test]
    fn the_count_is_of_forwards_that_bound() {
        let mut mgr = PortForwardManager::new();
        let id = mgr.add_local("127.0.0.1", 8080, "localhost", 80).id.clone();
        assert_eq!(
            mgr.active_count(),
            Some((0, 1)),
            "a starting forward is not a working one"
        );

        mgr.set_state(&id, ForwardState::Active { connections: 0 });
        assert_eq!(mgr.active_count(), Some((1, 1)));

        mgr.set_state(
            &id,
            ForwardState::Failed {
                reason: "port 8080 already in use".into(),
            },
        );
        assert_eq!(
            mgr.active_count(),
            Some((0, 1)),
            "a failed forward is still a rule, and still not active"
        );
    }

    /// A session with no forwards says nothing, rather than `0 of 0`.
    #[test]
    fn no_forwards_is_not_a_count() {
        assert_eq!(PortForwardManager::new().active_count(), None);
    }

    #[test]
    fn test_remove_forward() {
        let mut mgr = PortForwardManager::new();
        mgr.add_local("127.0.0.1", 8080, "localhost", 80);
        let id = mgr.forwards[0].id.clone();
        assert!(mgr.remove(&id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut mgr = PortForwardManager::new();
        assert!(!mgr.remove("nonexistent-id"));
    }

    #[test]
    fn test_remove_preserves_other_forwards() {
        let mut mgr = PortForwardManager::new();
        mgr.add_local("127.0.0.1", 8080, "localhost", 80);
        mgr.add_local("127.0.0.1", 3306, "localhost", 3306);
        let id = mgr.forwards[0].id.clone();
        mgr.remove(&id);
        assert_eq!(mgr.forwards.len(), 1);
        assert_eq!(mgr.forwards[0].bind_port, 3306);
    }

    #[test]
    fn test_parse_forward_spec_local() {
        let result = parse_forward_spec("L:8080:localhost:80");
        assert!(result.is_some());
        let (dir, bind, host, target) = result.unwrap();
        assert_eq!(dir, ForwardDirection::Local);
        assert_eq!(bind, 8080);
        assert_eq!(host, "localhost");
        assert_eq!(target, 80);
    }

    #[test]
    fn test_parse_forward_spec_remote() {
        let result = parse_forward_spec("R:3306:localhost:3306");
        assert!(result.is_some());
        let (dir, ..) = result.unwrap();
        assert_eq!(dir, ForwardDirection::Remote);
    }

    #[test]
    fn test_parse_forward_spec_invalid() {
        assert!(parse_forward_spec("X:8080:localhost:80").is_none());
        assert!(parse_forward_spec("L:abc:localhost:80").is_none());
        assert!(parse_forward_spec("L:8080:localhost").is_none());
        assert!(parse_forward_spec("").is_none());
    }

    #[test]
    fn test_direction_display() {
        assert_eq!(format!("{}", ForwardDirection::Local), "L");
        assert_eq!(format!("{}", ForwardDirection::Remote), "R");
    }

    #[test]
    fn test_select_navigation() {
        let mut mgr = PortForwardManager::new();
        mgr.add_local("127.0.0.1", 8080, "localhost", 80);
        mgr.add_local("127.0.0.1", 3306, "localhost", 3306);
        assert_eq!(mgr.selected, 0);
        mgr.select_next();
        assert_eq!(mgr.selected, 1);
        mgr.select_next();
        assert_eq!(mgr.selected, 0);
        mgr.select_prev();
        assert_eq!(mgr.selected, 1);
    }
}
