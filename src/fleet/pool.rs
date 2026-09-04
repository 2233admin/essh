//! Warm SSH connections for the fleet sampler. SPEC §6.1 step 1.
//!
//! One connection per host, opened on first use and reopened when it breaks.
//! `sampler`'s module doc carries the measurement behind that choice; this is
//! the part that holds the sockets.
//!
//! Three rules the pool exists to enforce:
//!
//! 1. **It never prompts.** A background sweep that asks for a key passphrase
//!    would do it from a task with no terminal, behind whatever screen is
//!    drawn. `Auth::for_sampling` refuses an encrypted key up front and the
//!    host reports `unreadable` with the reason, which is a row you can read
//!    rather than a hang you cannot.
//! 2. **It reuses a session's handle when there is one.** `SshSession.handle`
//!    is an `Arc` for exactly this, so a host you are attached to costs no
//!    second connection, no second auth, and no second entry in its `w`
//!    output.
//! 3. **A broken connection is dropped, not retried in place.** The next
//!    sweep opens a new one, and `sampler`'s back-off decides how soon that
//!    is.

use std::collections::HashMap;
use std::sync::Arc;

use russh::client::Handle;
use tokio::sync::Mutex;

use crate::ssh::{AuthMethod, ClientHandler, ConnectConfig, SshClient};

/// How a host is addressed in the pool. Name alone is not enough: two config
/// entries can point at one machine on different ports.
pub fn key(hostname: &str, port: u16) -> String {
    format!("{hostname}:{port}")
}

/// Auth the sampler is allowed to use.
///
/// Deliberately narrower than the interactive path. Password auth is absent
/// because there is nobody to type one, and an encrypted key is rejected
/// rather than unlocked, because unlocking it means a prompt.
pub struct Auth;

impl Auth {
    pub fn for_sampling(auth: &AuthMethod) -> Result<AuthMethod, String> {
        match auth {
            AuthMethod::Agent => Ok(AuthMethod::Agent),
            AuthMethod::KeyFile { path, passphrase } => {
                match russh::keys::load_secret_key(path, passphrase.as_deref()) {
                    Ok(_) => Ok(auth.clone()),
                    Err(_) => Err(format!(
                        "{} needs a passphrase; background sampling cannot ask for one",
                        path.display()
                    )),
                }
            }
            AuthMethod::Password(_) => {
                Err("password auth cannot be used for background sampling".into())
            }
        }
    }
}

#[derive(Default)]
pub struct FleetPool {
    warm: Arc<Mutex<HashMap<String, Arc<Handle<ClientHandler>>>>>,
}

impl FleetPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hand the pool a handle it did not open. Called when a session
    /// connects, so the sampler stops opening its own connection to a host
    /// you are already on.
    pub async fn adopt(&self, hostname: &str, port: u16, handle: Arc<Handle<ClientHandler>>) {
        self.warm.lock().await.insert(key(hostname, port), handle);
    }

    pub async fn forget(&self, hostname: &str, port: u16) {
        self.warm.lock().await.remove(&key(hostname, port));
    }

    /// A cheap clone that a spawned task can own.
    pub fn share(&self) -> Self {
        Self {
            warm: self.warm.clone(),
        }
    }

    async fn handle_for(&self, cfg: &ConnectConfig) -> Result<Arc<Handle<ClientHandler>>, String> {
        let k = key(&cfg.hostname, cfg.port);
        if let Some(h) = self.warm.lock().await.get(&k) {
            return Ok(h.clone());
        }
        // The lock is released across the handshake. Two sweeps racing on one
        // host would open two connections and keep the later one, which costs
        // a handshake and nothing else; holding the lock across a connect
        // would stall every other host behind the slowest one.
        let (session, _fingerprint, _banner) =
            SshClient::connect(cfg).await.map_err(|e| e.to_string())?;
        let handle = session.handle.clone();
        self.warm.lock().await.insert(k, handle.clone());
        Ok(handle)
    }

    /// Run one command and return its output.
    ///
    /// A failure drops the cached handle: a channel that will not open is
    /// usually a connection that has gone, and keeping it means every
    /// subsequent sweep fails the same way against the same dead socket.
    pub async fn exec(&self, cfg: &ConnectConfig, command: &str) -> Result<String, String> {
        let handle = self.handle_for(cfg).await?;
        match exec_on(&handle, command).await {
            Ok(out) => Ok(out),
            Err(e) => {
                self.forget(&cfg.hostname, cfg.port).await;
                Err(e)
            }
        }
    }
}

async fn exec_on(handle: &Handle<ClientHandler>, command: &str) -> Result<String, String> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("channel: {e}"))?;
    channel
        .exec(true, command.as_bytes())
        .await
        .map_err(|e| format!("exec: {e}"))?;

    let mut out = Vec::new();
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
            russh::ChannelMsg::Eof | russh::ChannelMsg::Close => break,
            russh::ChannelMsg::ExitStatus { .. } => break,
            _ => {}
        }
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The rule that matters most here. A sweep runs on a timer from a task
    /// with no terminal; a passphrase prompt from one would hang behind
    /// whatever screen is drawn, with no way to answer it.
    #[test]
    fn sampling_refuses_auth_it_would_have_to_prompt_for() {
        let err = Auth::for_sampling(&AuthMethod::Password("hunter2".into()))
            .expect_err("password auth was accepted for background sampling");
        assert!(err.contains("password"), "{err}");

        let missing = Auth::for_sampling(&AuthMethod::KeyFile {
            path: PathBuf::from("/nonexistent/id_ed25519"),
            passphrase: None,
        })
        .expect_err("an unreadable key was accepted");
        assert!(missing.contains("passphrase"), "{missing}");
    }

    #[test]
    fn agent_auth_is_fine_because_it_asks_nobody() {
        assert!(matches!(
            Auth::for_sampling(&AuthMethod::Agent),
            Ok(AuthMethod::Agent)
        ));
    }

    /// Two config entries can name one machine on two ports, and they are two
    /// hosts as far as the fleet is concerned.
    #[test]
    fn the_pool_key_separates_ports() {
        assert_ne!(key("127.0.0.1", 22), key("127.0.0.1", 2222));
    }
}
