pub mod pool;
pub mod sampler;

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Result of a single TCP probe to a host.
#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub online: bool,
    pub latency_ms: Option<f64>,
    #[allow(dead_code)]
    pub last_probed: Instant,
}

/// Per-host probe state including latency history for sparklines.
#[derive(Clone, Debug)]
pub struct HostProbeState {
    pub result: ProbeResult,
    pub latency_history: Vec<u64>,
    pub history_capacity: usize,
    /// Whether this host has ever answered in this run.
    ///
    /// It is the only thing that separates "the connection reset" from "it
    /// has never answered", and SPEC §1 hangs two different words, two
    /// glyphs and two recovery keys on that difference. Deriving it from a
    /// non-empty latency history would work today and break the first time
    /// a probe succeeds without recording a latency.
    pub ever_online: bool,
}

impl HostProbeState {
    pub fn new(capacity: usize) -> Self {
        Self {
            result: ProbeResult {
                online: false,
                latency_ms: None,
                last_probed: Instant::now(),
            },
            latency_history: Vec::with_capacity(capacity),
            history_capacity: capacity,
            ever_online: false,
        }
    }

    pub fn record(&mut self, result: ProbeResult) {
        self.ever_online |= result.online;
        if let Some(ms) = result.latency_ms {
            self.latency_history.push(ms as u64);
            if self.latency_history.len() > self.history_capacity {
                self.latency_history.remove(0);
            }
        }
        self.result = result;
    }
}

/// Fleet-wide probe state, keyed by "hostname:port".
pub struct FleetProber {
    pub states: HashMap<String, HostProbeState>,
    pub probe_timeout: Duration,
    pub history_capacity: usize,
    pub last_probe_cycle: Option<Instant>,
    pub probe_interval: Duration,
}

impl FleetProber {
    pub fn new(probe_interval_secs: u64, probe_timeout_secs: u64, history_capacity: usize) -> Self {
        Self {
            states: HashMap::new(),
            probe_timeout: Duration::from_secs(probe_timeout_secs),
            history_capacity,
            last_probe_cycle: None,
            probe_interval: Duration::from_secs(probe_interval_secs),
        }
    }

    /// Returns true if enough time has elapsed since the last probe cycle.
    pub fn should_probe(&self) -> bool {
        match self.last_probe_cycle {
            None => true,
            Some(last) => last.elapsed() >= self.probe_interval,
        }
    }

    pub fn probe_timeout(&self) -> Duration {
        self.probe_timeout
    }

    pub fn mark_probe_started(&mut self) {
        self.last_probe_cycle = Some(Instant::now());
    }

    /// Probe a single host via TCP connect. Returns the result.
    pub async fn probe_host(hostname: &str, port: u16, timeout: Duration) -> ProbeResult {
        let addr = format!("{}:{}", hostname, port);
        let start = Instant::now();
        let result = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await;
        let elapsed = start.elapsed();

        match result {
            Ok(Ok(_stream)) => ProbeResult {
                online: true,
                latency_ms: Some(elapsed.as_secs_f64() * 1000.0),
                last_probed: Instant::now(),
            },
            _ => ProbeResult {
                online: false,
                latency_ms: None,
                last_probed: Instant::now(),
            },
        }
    }

    pub async fn probe_hosts(
        hosts: Vec<(String, u16)>,
        timeout: Duration,
    ) -> Vec<(String, u16, ProbeResult)> {
        let mut handles = Vec::with_capacity(hosts.len());
        for (hostname, port) in hosts {
            handles.push(tokio::spawn(async move {
                let result = Self::probe_host(&hostname, port, timeout).await;
                (hostname, port, result)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    pub fn record_probe_results(&mut self, results: Vec<(String, u16, ProbeResult)>) {
        let capacity = self.history_capacity;

        for (hostname, port, result) in results {
            let key = format!("{}:{}", hostname, port);
            let state = self
                .states
                .entry(key)
                .or_insert_with(|| HostProbeState::new(capacity));
            state.record(result);
        }
    }

    /// Get probe state for a specific host.
    pub fn get_state(&self, hostname: &str, port: u16) -> Option<&HostProbeState> {
        let key = format!("{}:{}", hostname, port);
        self.states.get(&key)
    }
}

/// Return the color name for a latency value using Netwatch thresholds.
/// green < 50 ms, yellow < 200 ms, red ≥ 200 ms
#[allow(dead_code)]
pub fn latency_color_class(ms: f64) -> &'static str {
    if ms < 50.0 {
        "green"
    } else if ms < 200.0 {
        "yellow"
    } else {
        "red"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_probe_state_history_capacity() {
        let mut state = HostProbeState::new(3);
        for i in 0..5 {
            state.record(ProbeResult {
                online: true,
                latency_ms: Some(i as f64 * 10.0),
                last_probed: Instant::now(),
            });
        }
        assert_eq!(state.latency_history.len(), 3);
        assert_eq!(state.latency_history, vec![20, 30, 40]);
    }

    #[test]
    fn test_host_probe_state_offline_no_history() {
        let mut state = HostProbeState::new(10);
        state.record(ProbeResult {
            online: true,
            latency_ms: Some(42.0),
            last_probed: Instant::now(),
        });
        state.record(ProbeResult {
            online: false,
            latency_ms: None,
            last_probed: Instant::now(),
        });
        // Offline probe doesn't add to history
        assert_eq!(state.latency_history.len(), 1);
        assert!(!state.result.online);
    }

    #[test]
    fn test_latency_color_class() {
        assert_eq!(latency_color_class(10.0), "green");
        assert_eq!(latency_color_class(49.9), "green");
        assert_eq!(latency_color_class(50.0), "yellow");
        assert_eq!(latency_color_class(199.9), "yellow");
        assert_eq!(latency_color_class(200.0), "red");
        assert_eq!(latency_color_class(1000.0), "red");
    }

    #[test]
    fn test_fleet_prober_should_probe_initially() {
        let prober = FleetProber::new(60, 5, 30);
        assert!(prober.should_probe());
    }

    #[test]
    fn test_fleet_prober_get_state_missing() {
        let prober = FleetProber::new(60, 5, 30);
        assert!(prober.get_state("nonexistent", 22).is_none());
    }

    #[test]
    fn test_fleet_prober_record_probe_results_updates_state() {
        let mut prober = FleetProber::new(60, 5, 3);
        prober.record_probe_results(vec![(
            "web.example.com".to_string(),
            22,
            ProbeResult {
                online: true,
                latency_ms: Some(12.0),
                last_probed: Instant::now(),
            },
        )]);

        let state = prober
            .get_state("web.example.com", 22)
            .expect("probe state");
        assert!(state.result.online);
        assert_eq!(state.latency_history, vec![12]);
    }

    #[tokio::test]
    async fn test_probe_host_unreachable() {
        // Probe a host that should not be reachable (reserved IP)
        let result = FleetProber::probe_host("192.0.2.1", 1, Duration::from_millis(100)).await;
        assert!(!result.online);
        assert!(result.latency_ms.is_none());
    }
}

/// The sampler against real hosts. SPEC §6.1.
///
/// The unit tests in `sampler` drive the parser and the schedule off canned
/// output, which is the right place to assert both. They cannot tell you
/// whether the command essh sends produces output essh can read, over a
/// connection essh opens. That is the failure this module exists for, and it
/// is the one that happened: the sampler held eight connections to the
/// fixture fleet and every column still read a dash.
///
/// Requires the fixture fleet. Without it these say so and skip, rather than
/// failing on a machine with no podman.
#[cfg(test)]
mod live {
    use std::time::{Duration, Instant};

    use super::pool::FleetPool;
    use super::sampler::{FleetSampler, Outcome};
    use crate::ssh::{AuthMethod, ConnectConfig};

    const FLEET: [(&str, &str); 3] = [
        ("web-02", "127.0.0.12"),
        ("db-01", "127.0.0.14"),
        ("build-01", "127.0.0.18"),
    ];

    fn key() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").expect("HOME")).join(".ssh/id_ed25519")
    }

    fn reachable(ip: &str) -> bool {
        use std::net::{SocketAddr, TcpStream};
        let addr: SocketAddr = format!("{ip}:2222").parse().expect("addr");
        TcpStream::connect_timeout(&addr, Duration::from_millis(600)).is_ok()
    }

    fn cfg(ip: &str) -> ConnectConfig {
        ConnectConfig {
            hostname: ip.to_string(),
            port: 2222,
            username: "matt".into(),
            auth: AuthMethod::KeyFile {
                path: key(),
                passphrase: None,
            },
            timeout: Duration::from_secs(10),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_sampler_reads_real_hosts_with_no_session_open() {
        if !FLEET.iter().all(|(_, ip)| reachable(ip)) {
            eprintln!("SKIP: fixture fleet not up; run ./scripts/fleet.sh up");
            return;
        }

        let pool = FleetPool::new();
        let mut sampler = FleetSampler::new(Duration::from_millis(1), 8, 8);
        let names: Vec<String> = FLEET.iter().map(|(n, _)| n.to_string()).collect();

        // Two rounds: a CPU percentage needs two `/proc/stat` reads to
        // difference, so one round would pass with `cpu_pct: None` and prove
        // nothing.
        let mut last = Vec::new();
        for round in 0..2 {
            let due = sampler.due(&names, Instant::now());
            assert_eq!(
                due.len(),
                FLEET.len(),
                "round {round}: not every host was due"
            );

            let mut raw = Vec::new();
            for sweep in due {
                let ip = FLEET
                    .iter()
                    .find(|(n, _)| *n == sweep.host)
                    .map(|(_, ip)| *ip)
                    .expect("host");
                raw.push((sweep.host, pool.exec(&cfg(ip), &sweep.command).await));
            }

            last.clear();
            for (host, out) in raw {
                let outcome = sampler.record(&host, out, Instant::now());
                last.push((host, outcome));
            }
            if round == 0 {
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
        }

        for (host, outcome) in &last {
            let Outcome::Read(sample) = outcome else {
                panic!("{host}: the sampler could not read a live host: {outcome:?}");
            };
            assert!(
                sample.cpu_pct.is_some(),
                "{host}: no CPU after two sweeps — the sweep ran but nothing parsed"
            );
            assert!(sample.mem_pct.is_some_and(|m| m > 0.0), "{host}: no memory");
            assert!(sample.disk_pct.is_some_and(|d| d > 0.0), "{host}: no disk");
            assert!(sample.load1.is_some(), "{host}: no load average");
            assert!(
                sample.cores.is_some_and(|c| c > 0),
                "{host}: no core count, so `load` cannot be related to `cpu%`"
            );
            assert!(
                !sample.processes.is_empty(),
                "{host}: no processes, so the health screen cannot attribute load"
            );
            let cpu = sample.cpu_pct.expect("cpu");
            assert!(
                (0.0..=100.0).contains(&cpu),
                "{host}: cpu {cpu} is not a percentage"
            );
        }
    }

    /// SPEC §6.1 rule 2, and the reason the pool exists. A second sweep must
    /// not cost a second handshake.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_sweep_reuses_the_connection() {
        if !reachable(FLEET[0].1) {
            eprintln!("SKIP: fixture fleet not up");
            return;
        }
        let pool = FleetPool::new();
        let c = cfg(FLEET[0].1);

        let first = Instant::now();
        pool.exec(&c, "echo one").await.expect("first exec");
        let cold = first.elapsed();

        let second = Instant::now();
        pool.exec(&c, "echo two").await.expect("second exec");
        let warm = second.elapsed();

        assert!(
            warm < cold,
            "the second sweep cost as much as the first ({warm:?} vs {cold:?}): \
             the connection is not being reused"
        );
    }
}

/// Remote port forwards, against a real sshd. SPEC §6.4.
///
/// The router's unit tests prove the lookup. They cannot prove that the
/// server accepts the request, that the channel comes back, or that the bytes
/// arrive — which is the whole feature, and all three failed silently before
/// this existed: `server_channel_open_forwarded_tcpip` took the channel and
/// dropped it, so the far end hung rather than erroring.
#[cfg(test)]
mod remote_forward_live {
    use std::time::Duration;

    use crate::ssh::{AuthMethod, ConnectConfig, SshClient};

    fn cfg() -> ConnectConfig {
        ConnectConfig {
            hostname: "127.0.0.12".into(),
            port: 2222,
            username: "matt".into(),
            auth: AuthMethod::KeyFile {
                path: std::path::PathBuf::from(std::env::var("HOME").expect("HOME"))
                    .join(".ssh/id_ed25519"),
                passphrase: None,
            },
            timeout: Duration::from_secs(10),
        }
    }

    fn reachable() -> bool {
        use std::net::{SocketAddr, TcpStream};
        let addr: SocketAddr = "127.0.0.12:2222".parse().expect("addr");
        TcpStream::connect_timeout(&addr, Duration::from_millis(600)).is_ok()
    }

    /// The end-to-end claim: the server listens, and what it accepts reaches
    /// a local socket.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_remote_forward_carries_bytes_from_the_server_to_us() {
        if !reachable() {
            eprintln!("SKIP: fixture fleet not up; run ./scripts/fleet.sh up");
            return;
        }

        // A local echo server standing in for whatever the forward points at.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a local target");
        let target_port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 64];
                    if let Ok(n) = sock.read(&mut buf).await {
                        let _ = sock.write_all(&buf[..n]).await;
                        let _ = sock.flush().await;
                    }
                });
            }
        });

        let (session, _, _) = SshClient::connect(&cfg()).await.expect("connect");
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        // Port 0 asks the server to choose, so the test cannot collide with
        // anything already listening in the container.
        let bound = session
            .request_remote_forward(
                &cfg(),
                "localhost",
                0,
                "127.0.0.1",
                target_port,
                std::sync::Arc::clone(&counter),
            )
            .await
            .expect("the server accepted the forward");
        assert!(bound > 0, "the server bound port {bound}");

        // Reach it from the far side: a command *on the host* connecting to
        // the port the host is now listening on. That is the direction the
        // feature exists for, and the one that was silently broken.
        let out = session
            .handle
            .channel_open_session()
            .await
            .expect("channel");
        let mut out = out;
        // perl rather than nc: the fixture image has no netcat, and bash's
        // /dev/tcp is a bashism the container's `sh` does not have. This is
        // the one place the test has to reach *from* the host, so it uses
        // what the host actually has.
        let probe = format!(
            r#"perl -e 'use IO::Socket::INET; my $s = IO::Socket::INET->new("127.0.0.1:{bound}") or die; print $s "hello"; $s->flush; my $r; $s->sysread($r, 16); print $r;'"#
        );
        out.exec(true, probe.as_bytes()).await.expect("exec");

        let mut echoed = Vec::new();
        while let Some(msg) = out.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } => echoed.extend_from_slice(&data),
                russh::ChannelMsg::Eof | russh::ChannelMsg::Close => break,
                _ => {}
            }
        }

        let echoed = String::from_utf8_lossy(&echoed);
        assert!(
            echoed.contains("hello"),
            "the forward did not carry the bytes back; got {echoed:?}"
        );
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "the connection was not counted"
        );

        session
            .cancel_remote_forward("localhost", bound)
            .await
            .expect("cancel");
        session.close().await.ok();
    }

    /// A cancelled forward stops being routed, so a connection racing the
    /// cancel is refused rather than proxied to a rule that is gone.
    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_a_forward_stops_the_server_listening() {
        if !reachable() {
            eprintln!("SKIP: fixture fleet not up");
            return;
        }
        let (session, _, _) = SshClient::connect(&cfg()).await.expect("connect");
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let bound = session
            .request_remote_forward(&cfg(), "localhost", 0, "127.0.0.1", 9, counter)
            .await
            .expect("forward");
        session
            .cancel_remote_forward("localhost", bound)
            .await
            .expect("cancel");

        let probe = session
            .handle
            .channel_open_session()
            .await
            .expect("channel");
        let mut probe = probe;
        probe
            .exec(
                true,
                format!(
                    r#"perl -e 'use IO::Socket::INET; print IO::Socket::INET->new("127.0.0.1:{bound}") ? "open" : "shut";'"#
                )
                .as_bytes(),
            )
            .await
            .expect("exec");
        let mut out = Vec::new();
        while let Some(msg) = probe.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
                russh::ChannelMsg::Eof | russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        let out = String::from_utf8_lossy(&out);
        assert!(
            out.contains("shut"),
            "the server is still listening on {bound} after the cancel: {out:?}"
        );
        session.close().await.ok();
    }
}
