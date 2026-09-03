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
