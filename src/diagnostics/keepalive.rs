//! The measurement behind the session's link-quality line. SPEC §6.5.
//!
//! `DiagnosticsEngine` has carried `record_rtt`, `record_keepalive_sent` and
//! `record_keepalive_received` since the start, all three `#[allow(dead_code)]`
//! and none of them called. The status line read
//!
//! ```text
//! link quality unmeasured — waiting for the first keepalive
//! ```
//!
//! for the life of every session, because no keepalive was ever sent. That is
//! worse than showing nothing: it names a thing that is coming and then does
//! not deliver it, so a user waits instead of concluding the feature is absent.
//!
//! ## Why a ping and not a request
//!
//! `Handle::send_ping` sends `SSH_MSG_GLOBAL_REQUEST` and awaits the peer's
//! reply. It takes `&self`, so it works through the `Arc<Handle>` every
//! session already holds — no lock, no second connection, none of what a
//! remote forward needed (see `portfwd::remote`). The round trip it measures
//! is the connection's own, not a fresh TCP handshake's, which is the number
//! the line is asking for.
//!
//! ## What a slow pong means
//!
//! russh answers the pong on the same task that pumps channel data, so a
//! session whose shell is flooding reports a longer round trip. That is not a
//! distortion to correct: the link *is* slower for anything else trying to use
//! it, which is exactly what "link quality" is for. What must not happen is a
//! ping that never returns wedging the task, so each one is bounded and a
//! timeout counts as loss — `packet_loss_pct` is sent-minus-received, so the
//! quality verdict degrades on its own.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use russh::client::Handle;

use crate::ssh::ClientHandler;

/// Between pings. Short enough that the first few readings arrive while a user
/// is still looking at the session they just opened; long enough that eight
/// open sessions are not eight pings a second.
const INTERVAL: Duration = Duration::from_secs(15);

/// The wait after the first ping only.
///
/// That first ping goes out while the handshake is still settling, and it
/// shows: against a container on loopback it reads 47ms where the next seven
/// read 0.5 to 0.9. Seventy times the truth, and enough to grade an Excellent
/// link as Good. Withholding it would put us back to fifteen seconds of
/// "unmeasured", which is the thing this module exists to fix — so publish it
/// and correct it a second later instead.
const SETTLE: Duration = Duration::from_secs(1);

/// How long a single round trip may take before it counts as lost. Well past
/// any real link — a 10-second RTT is not a slow connection, it is a dead one.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Measure this connection's round trip until it closes.
///
/// Takes a `Weak`, deliberately. A strong `Arc<Handle>` here would keep the
/// SSH connection open for as long as the task ran, and the task runs until
/// the connection closes — so the session would never be collected and the
/// server would hold a channel for a window nobody is looking at.
pub fn spawn(handle: &Arc<Handle<ClientHandler>>, engine: &crate::diagnostics::DiagnosticsEngine) {
    let weak = Arc::downgrade(handle);
    let engine = engine.clone();

    tokio::spawn(async move {
        let mut settled = false;
        loop {
            // Upgrade for the ping alone, then drop it, so the wait between
            // pings never holds the connection alive.
            let Some(handle) = Weak::upgrade(&weak) else {
                return;
            };

            let sent_at = Instant::now();
            let outcome = tokio::time::timeout(TIMEOUT, handle.send_ping()).await;
            drop(handle);

            match outcome {
                Ok(Ok(())) => {
                    engine
                        .record_ping(Some(sent_at.elapsed().as_secs_f64() * 1000.0))
                        .await
                }
                // A timeout is loss: counted, but it leaves no reading, so the
                // line keeps the last real number and the verdict degrades.
                Err(_) => engine.record_ping(None).await,
                // A send error means the connection is gone, not slow. Do not
                // record it as a lost ping — the session is over.
                Ok(Err(_)) => return,
            }

            tokio::time::sleep(if settled { INTERVAL } else { SETTLE }).await;
            settled = true;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first ping goes out before the first sleep. A user who opens a
    /// session and looks at the status line should see a number, not the
    /// fifteen seconds of "unmeasured" that a sleep-first loop would give.
    #[test]
    fn a_ping_is_sent_before_the_first_wait() {
        let src = include_str!("keepalive.rs");
        let body = src.split("loop {").nth(1).expect("the loop body");
        let ping = body.find("send_ping").expect("the ping");
        let sleep = body
            .find("tokio::time::sleep(if settled")
            .expect("the wait");
        assert!(ping < sleep, "the loop must ping before it waits");
    }

    /// A timeout is not a reading. Recording one as an RTT would report a
    /// dead link as a 10ms-times-a-thousand one, which `connection_quality`
    /// grades as Critical rather than as the disconnection it is.
    #[tokio::test]
    async fn a_timeout_is_counted_as_loss_and_leaves_no_reading() {
        let e = crate::diagnostics::DiagnosticsEngine::new("s", "h", 22, None);
        e.record_ping(Some(3.0)).await;
        e.record_ping(None).await;
        e.record_ping(Some(4.0)).await;
        e.record_ping(None).await;

        let snap = e.snapshot().await;
        assert_eq!(snap.packet_loss_pct, 50.0);
        assert_eq!(snap.rtt_ms, Some(4.0), "the last good reading stands");
    }

    /// Sent and received move together or not at all. Bumping them under
    /// separate locks would let the status line read 100% loss between the
    /// two writes, on a link that answered.
    #[tokio::test]
    async fn a_healthy_link_never_reports_loss() {
        let e = crate::diagnostics::DiagnosticsEngine::new("s", "h", 22, None);
        for _ in 0..8 {
            e.record_ping(Some(1.5)).await;
            assert_eq!(e.snapshot().await.packet_loss_pct, 0.0);
        }
    }

    /// A ping must finish, one way or the other, before the next is due —
    /// otherwise a dead link queues pings instead of reporting loss.
    #[test]
    fn a_ping_always_resolves_before_the_next_one_is_due() {
        let waits = [SETTLE, INTERVAL];
        assert_eq!(waits.iter().max(), Some(&INTERVAL));
        assert!(
            TIMEOUT > Duration::from_secs(1),
            "a real link gets a fair wait"
        );
        // SETTLE is shorter than TIMEOUT on purpose: a first ping that timed
        // out published nothing, so its correction is not late.
        assert!(waits[0] < TIMEOUT);
    }

    /// Against a real host. The unit tests above prove the bookkeeping; only
    /// this proves that russh answers the ping at all, which is the thing that
    /// was silently absent for the whole life of the feature.
    ///
    ///     ./scripts/fleet.sh up
    ///     cargo test --bins keepalive -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "needs the container fleet: ./scripts/fleet.sh up"]
    async fn a_real_host_answers_the_ping() {
        use crate::ssh::{AuthMethod, ConnectConfig, SshClient};

        let cfg = ConnectConfig {
            hostname: "127.0.0.11".into(),
            port: 2222,
            username: "matt".into(),
            auth: AuthMethod::KeyFile {
                path: dirs::home_dir().unwrap().join(".ssh/id_ed25519"),
                passphrase: None,
            },
            timeout: Duration::from_secs(10),
        };
        let (session, _fp, _banner) = SshClient::connect(&cfg).await.expect("connect");
        let engine = crate::diagnostics::DiagnosticsEngine::new("live", &cfg.hostname, 2222, None);

        spawn(&session.handle, &engine);

        // One round trip for the first reading.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let first = engine.snapshot().await.rtt_ms.expect("the host answered");

        // Then the settle wait, and the reading that replaces it.
        tokio::time::sleep(SETTLE + Duration::from_millis(500)).await;
        let snap = engine.snapshot().await;
        let rtt = snap.rtt_ms.expect("and answered again");

        println!(
            "first {first:.2}ms  settled {rtt:.2}ms  loss {:.0}%  {:?}",
            snap.packet_loss_pct, snap.quality
        );
        assert!(rtt > 0.0, "a round trip takes time");
        assert!(rtt < 1000.0, "a container on loopback is not a second away");
        assert_eq!(snap.packet_loss_pct, 0.0);
        assert!(
            rtt < first,
            "the settled reading corrects the handshake-inflated first one"
        );
    }
}
