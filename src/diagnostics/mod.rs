pub mod keepalive;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub session_id: String,
    pub connected_at: DateTime<Utc>,
    pub hostname: String,
    pub port: u16,
    pub server_banner: Option<String>,
    pub kex_algorithm: Option<String>,
    pub cipher: Option<String>,
    pub mac: Option<String>,
    pub compression: Option<String>,
    pub auth_method: Option<String>,
    pub rtt_ms: Option<f64>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub keepalive_sent: u64,
    pub keepalive_received: u64,
    pub channels_active: u32,
    pub last_rekey_at: Option<DateTime<Utc>>,
    pub bytes_since_rekey: u64,
    /// Byte counts and the clock at the last rate mark. See `roll_rates`.
    pub rate_mark: (u64, u64, DateTime<Utc>),
    /// Bytes per second over the last marked window, not over the session.
    pub rate_up_bps: f64,
    pub rate_down_bps: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConnectionQuality {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub timestamp: String,
    pub session_id: String,
    pub rtt_ms: Option<f64>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub throughput_up_bps: f64,
    pub throughput_down_bps: f64,
    pub packet_loss_pct: f64,
    pub quality: ConnectionQuality,
    pub uptime_secs: i64,
    pub channels_active: u32,
}

/// Clone shares the metrics rather than copying them: the `Arc<RwLock<_>>` is
/// the point, and `metrics()` already hands that same handle out. A clone
/// writes to the same log file, which is what a second writer on one session
/// should do.
#[derive(Clone)]
pub struct DiagnosticsEngine {
    metrics: Arc<RwLock<SessionMetrics>>,
    log_file: Option<PathBuf>,
}

impl SessionMetrics {
    pub fn new(session_id: String, hostname: String, port: u16) -> Self {
        Self {
            session_id,
            connected_at: Utc::now(),
            hostname,
            port,
            server_banner: None,
            kex_algorithm: None,
            cipher: None,
            mac: None,
            compression: None,
            auth_method: None,
            rtt_ms: None,
            bytes_sent: 0,
            bytes_received: 0,
            keepalive_sent: 0,
            keepalive_received: 0,
            channels_active: 0,
            last_rekey_at: None,
            bytes_since_rekey: 0,
            rate_mark: (0, 0, Utc::now()),
            rate_up_bps: 0.0,
            rate_down_bps: 0.0,
        }
    }

    pub fn uptime(&self) -> chrono::Duration {
        Utc::now() - self.connected_at
    }

    pub fn packet_loss_pct(&self) -> f64 {
        if self.keepalive_sent > 0 {
            (1.0 - self.keepalive_received as f64 / self.keepalive_sent as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn connection_quality(&self) -> ConnectionQuality {
        let rtt = self.rtt_ms.unwrap_or(0.0);
        let loss = self.packet_loss_pct();

        if rtt < 20.0 && loss < 1.0 {
            ConnectionQuality::Excellent
        } else if rtt < 50.0 && loss < 3.0 {
            ConnectionQuality::Good
        } else if rtt < 100.0 && loss < 5.0 {
            ConnectionQuality::Fair
        } else if rtt < 200.0 && loss < 10.0 {
            ConnectionQuality::Poor
        } else {
            ConnectionQuality::Critical
        }
    }

    /// Close the current rate window and open a new one.
    ///
    /// `bytes_sent / uptime` is a lifetime average, and a lifetime average is
    /// not a rate. A session that pulled 10MB in its first second and has been
    /// idle for an hour reports 2.8KB/s, steadily, forever — the same mistake
    /// `ps` makes with `%CPU`, and it sits on the same line as an RTT that
    /// *is* instantaneous, so the two read as if they were measured alike.
    ///
    /// The keepalive calls this, which sets the window: one second for the
    /// first, then fifteen. Coarse, but it is a rate.
    pub fn roll_rates(&mut self) {
        let (sent, received, at) = self.rate_mark;
        let secs = (Utc::now() - at).num_milliseconds() as f64 / 1000.0;
        if secs > 0.0 {
            self.rate_up_bps = self.bytes_sent.saturating_sub(sent) as f64 / secs;
            self.rate_down_bps = self.bytes_received.saturating_sub(received) as f64 / secs;
        }
        self.rate_mark = (self.bytes_sent, self.bytes_received, Utc::now());
    }
}

impl DiagnosticsEngine {
    pub fn new(session_id: &str, hostname: &str, port: u16, log_dir: Option<&Path>) -> Self {
        let metrics = SessionMetrics::new(session_id.to_string(), hostname.to_string(), port);
        let log_file = log_dir.map(|dir| dir.join(format!("{}.jsonl", session_id)));

        Self {
            metrics: Arc::new(RwLock::new(metrics)),
            log_file,
        }
    }

    pub fn metrics(&self) -> Arc<RwLock<SessionMetrics>> {
        Arc::clone(&self.metrics)
    }

    pub async fn snapshot(&self) -> DiagnosticsSnapshot {
        let m = self.metrics.read().await;
        let uptime = m.uptime();

        DiagnosticsSnapshot {
            timestamp: Utc::now().to_rfc3339(),
            session_id: m.session_id.clone(),
            rtt_ms: m.rtt_ms,
            bytes_sent: m.bytes_sent,
            bytes_received: m.bytes_received,
            throughput_up_bps: m.rate_up_bps,
            throughput_down_bps: m.rate_down_bps,
            packet_loss_pct: m.packet_loss_pct(),
            quality: m.connection_quality(),
            uptime_secs: uptime.num_seconds(),
            channels_active: m.channels_active,
        }
    }

    pub async fn record_bytes_sent(&self, n: u64) {
        let mut m = self.metrics.write().await;
        m.bytes_sent += n;
    }

    pub async fn record_bytes_received(&self, n: u64) {
        let mut m = self.metrics.write().await;
        m.bytes_received += n;
    }

    /// One completed keepalive: `Some(rtt)` if the pong came back, `None` if
    /// it timed out.
    ///
    /// One method rather than the three this replaced (`record_rtt`,
    /// `record_keepalive_sent`, `record_keepalive_received`, all three dead
    /// since they were written). The count of pings sent and the count that
    /// answered are the numerator and denominator of `packet_loss_pct`, so
    /// bumping them under separate locks leaves a window where a reader sees
    /// one and not the other and reports loss on a healthy link.
    pub async fn record_ping(&self, rtt_ms: Option<f64>) {
        let mut m = self.metrics.write().await;
        // The ping's cadence is the throughput window's cadence. One task, one
        // clock: a separate timer for the rates would drift against this one
        // and put two numbers measured over different windows on one line.
        m.roll_rates();
        m.keepalive_sent += 1;
        if let Some(rtt) = rtt_ms {
            m.keepalive_received += 1;
            m.rtt_ms = Some(rtt);
        }
    }

    pub async fn set_connection_info(
        &self,
        server_banner: Option<String>,
        kex: Option<String>,
        cipher: Option<String>,
        mac: Option<String>,
        compression: Option<String>,
        auth_method: Option<String>,
    ) {
        let mut m = self.metrics.write().await;
        m.server_banner = server_banner;
        m.kex_algorithm = kex;
        m.cipher = cipher;
        m.mac = mac;
        m.compression = compression;
        m.auth_method = auth_method;
    }

    #[allow(dead_code)]
    pub async fn set_channels_active(&self, count: u32) {
        let mut m = self.metrics.write().await;
        m.channels_active = count;
    }

    pub async fn write_log_entry(&self) -> Result<(), std::io::Error> {
        let snapshot = self.snapshot().await;

        if let Some(ref path) = self.log_file {
            let line = serde_json::to_string(&snapshot).map_err(std::io::Error::other)?;
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_session_metrics_new() {
        let m = SessionMetrics::new("sess-1".into(), "host.example.com".into(), 22);
        assert_eq!(m.session_id, "sess-1");
        assert_eq!(m.hostname, "host.example.com");
        assert_eq!(m.port, 22);
        assert_eq!(m.bytes_sent, 0);
        assert_eq!(m.bytes_received, 0);
        assert_eq!(m.keepalive_sent, 0);
        assert_eq!(m.keepalive_received, 0);
        assert_eq!(m.channels_active, 0);
        assert_eq!(m.bytes_since_rekey, 0);
        assert!(m.rtt_ms.is_none());
        assert!(m.server_banner.is_none());
        assert!(m.kex_algorithm.is_none());
        assert!(m.cipher.is_none());
        assert!(m.mac.is_none());
        assert!(m.compression.is_none());
        assert!(m.auth_method.is_none());
        assert!(m.last_rekey_at.is_none());
    }

    #[test]
    fn test_packet_loss_zero_when_no_keepalives() {
        let m = SessionMetrics::new("s".into(), "h".into(), 22);
        assert_eq!(m.packet_loss_pct(), 0.0);
    }

    #[test]
    fn test_packet_loss_calculation() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.keepalive_sent = 10;
        m.keepalive_received = 8;
        let loss = m.packet_loss_pct();
        assert!((loss - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_connection_quality_excellent() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rtt_ms = Some(10.0);
        // 0% loss (no keepalives → 0.0)
        assert_eq!(m.connection_quality(), ConnectionQuality::Excellent);
    }

    #[test]
    fn test_connection_quality_good() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rtt_ms = Some(30.0);
        m.keepalive_sent = 100;
        m.keepalive_received = 98; // 2% loss
        assert_eq!(m.connection_quality(), ConnectionQuality::Good);
    }

    #[test]
    fn test_connection_quality_fair() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rtt_ms = Some(80.0);
        m.keepalive_sent = 100;
        m.keepalive_received = 96; // 4% loss
        assert_eq!(m.connection_quality(), ConnectionQuality::Fair);
    }

    #[test]
    fn test_connection_quality_poor() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rtt_ms = Some(150.0);
        m.keepalive_sent = 100;
        m.keepalive_received = 92; // 8% loss
        assert_eq!(m.connection_quality(), ConnectionQuality::Poor);
    }

    #[test]
    fn test_connection_quality_critical() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rtt_ms = Some(300.0);
        m.keepalive_sent = 100;
        m.keepalive_received = 85; // 15% loss
        assert_eq!(m.connection_quality(), ConnectionQuality::Critical);
    }

    /// The rate covers the window since the last mark, not the session.
    #[test]
    fn a_rate_is_measured_over_its_window_and_not_over_the_session() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);

        // Two seconds ago we had transferred nothing.
        m.rate_mark = (0, 0, Utc::now() - chrono::Duration::seconds(2));
        m.bytes_sent = 1024;
        m.bytes_received = 4096;
        m.roll_rates();
        assert!((m.rate_up_bps - 512.0).abs() < 1.0, "{}", m.rate_up_bps);
        assert!(
            (m.rate_down_bps - 2048.0).abs() < 4.0,
            "{}",
            m.rate_down_bps
        );

        // Two more seconds, and nothing moved. A lifetime average would still
        // report 256 B/s up; the window reports the truth.
        m.rate_mark.2 = Utc::now() - chrono::Duration::seconds(2);
        m.roll_rates();
        assert_eq!(m.rate_up_bps, 0.0);
        assert_eq!(m.rate_down_bps, 0.0);
    }

    /// A window of zero length must not divide by it.
    #[test]
    fn two_marks_in_the_same_instant_leave_the_last_rate_alone() {
        let mut m = SessionMetrics::new("s".into(), "h".into(), 22);
        m.rate_up_bps = 99.0;
        m.rate_mark = (0, 0, Utc::now());
        m.bytes_sent = 1024;
        m.roll_rates();
        assert!(m.rate_up_bps.is_finite());
    }

    #[tokio::test]
    async fn test_diagnostics_engine_new() {
        let engine = DiagnosticsEngine::new("sess-1", "host", 22, None);
        let metrics = engine.metrics();
        let m = metrics.read().await;
        assert_eq!(m.session_id, "sess-1");
        assert_eq!(m.hostname, "host");
        assert_eq!(m.port, 22);
    }

    #[tokio::test]
    async fn test_record_bytes() {
        let engine = DiagnosticsEngine::new("s", "h", 22, None);
        engine.record_bytes_sent(100).await;
        engine.record_bytes_sent(50).await;
        engine.record_bytes_received(200).await;

        let snap = engine.snapshot().await;
        assert_eq!(snap.bytes_sent, 150);
        assert_eq!(snap.bytes_received, 200);
    }

    #[tokio::test]
    async fn test_set_connection_info() {
        let engine = DiagnosticsEngine::new("s", "h", 22, None);
        engine
            .set_connection_info(
                Some("OpenSSH_9.0".into()),
                Some("curve25519-sha256".into()),
                Some("aes256-gcm".into()),
                Some("hmac-sha2-256".into()),
                Some("none".into()),
                Some("publickey".into()),
            )
            .await;

        let m = engine.metrics().read().await.clone();
        assert_eq!(m.server_banner.as_deref(), Some("OpenSSH_9.0"));
        assert_eq!(m.kex_algorithm.as_deref(), Some("curve25519-sha256"));
        assert_eq!(m.cipher.as_deref(), Some("aes256-gcm"));
        assert_eq!(m.mac.as_deref(), Some("hmac-sha2-256"));
        assert_eq!(m.compression.as_deref(), Some("none"));
        assert_eq!(m.auth_method.as_deref(), Some("publickey"));
    }

    #[tokio::test]
    async fn test_write_log_entry() {
        let dir = std::env::temp_dir().join(format!("diag-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let engine = DiagnosticsEngine::new("log-sess", "h", 22, Some(dir.as_path()));
        engine.record_bytes_sent(42).await;
        engine.write_log_entry().await.unwrap();

        let log_path = dir.join("log-sess.jsonl");
        assert!(log_path.exists());

        let contents = fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(parsed["session_id"], "log-sess");
        assert_eq!(parsed["bytes_sent"], 42);

        // cleanup
        let _ = fs::remove_dir_all(&dir);
    }
}
