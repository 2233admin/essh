//! Metrics for every configured host, not only the one you have a session
//! on. SPEC §6.1.
//!
//! ## Why this module exists
//!
//! The v2 hosts screen shows CPU, memory and disk for every host in the
//! config, and the health screen derives its attention list, its cards and
//! its busiest-process table from the same readings. The handoff presents all
//! of that as layout. It is not.
//!
//! Before this, essh collected metrics through
//! `monitor::HostMetricsCollector`, which needs an established
//! `russh::client::Handle` — one you only have for a host you opened a
//! session on. `divergence::collect` has the same constraint. That is why the
//! v0.3.3 screenshots show facets for nine hosts: nine sessions were open. The
//! only fleet-wide thing essh did was `fleet::FleetProber`, a TCP connect
//! every 60 seconds, which answers "is it up" and nothing else.
//!
//! ## The connection strategy, and the measurement behind it
//!
//! Warm connections, one per host, reopened on failure.
//!
//! The alternative is a handshake per poll. Measured against the fixture
//! fleet on loopback that is 140ms, which looks affordable and is not the
//! number that matters. Eleven hosts on a 5s cycle is two handshakes a
//! second; two hundred hosts is forty. Each one is a curve25519 exchange and
//! an ed25519 verify on both ends, so a dashboard left open becomes a steady
//! 40/s authentication load against the fleet it is supposed to be watching,
//! and it looks like a password-spray to anything watching the logs. Over a
//! 20ms link the handshake is also five round trips rather than one, so the
//! loopback figure understates it by about 2x before any of that.
//!
//! A warm `Arc<Handle>` costs one idle TCP connection and one channel per
//! sweep. `SshSession.handle` is already an `Arc` precisely so background
//! work can share it, so a host you have a session on costs nothing extra.
//!
//! ## What it will not do
//!
//! - **Sample every host every cycle regardless of size.** `concurrency`
//!   caps how many sweeps are in flight. Two hundred simultaneous execs every
//!   five seconds is a denial of service you wrote yourself.
//! - **Line the hosts up.** Each host's due time carries a jitter derived
//!   from its name, so a fleet does not arrive at its sshd in one burst. It
//!   is derived rather than random so a test can predict it.
//! - **Retry a dead host at the dashboard's rate.** Three consecutive
//!   failures move a host to `BACKOFF_INTERVAL` until it answers.
//! - **Report a zero it did not measure.** Every field is an `Option` and a
//!   failed sweep produces `Outcome::Unreadable` or `Outcome::Unreachable`,
//!   both of which carry the reason in words.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::monitor::{parser, Platform, ProcessInfo};

/// Consecutive failed sweeps before a host drops to `BACKOFF_INTERVAL`.
pub const BACKOFF_AFTER: u32 = 3;
/// Where a failing host goes. The reachability prober's own cycle, so a host
/// that comes back is noticed at the same rate either way.
pub const BACKOFF_INTERVAL: Duration = Duration::from_secs(60);
/// Fraction of the interval a host's due time is spread over.
const JITTER: f64 = 0.2;

/// One host's readings from one sweep.
///
/// Every field is an `Option` because every one of them can be missing on a
/// host that otherwise answered: a container with no `/proc/stat`, a `df` the
/// login shell cannot run, a first sweep with nothing to difference CPU
/// against.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub cpu_pct: Option<f64>,
    pub mem_pct: Option<f64>,
    /// The busiest mounted filesystem, which is the one worth alarming on.
    /// A host with a full `/boot` and an empty `/` is not healthy.
    pub disk_pct: Option<f64>,
    pub load1: Option<f64>,
    pub cores: Option<usize>,
    /// Top processes by CPU, for the health screen's fleet-wide list.
    pub processes: Vec<ProcessInfo>,
    /// One entry per controlling terminal on the host.
    ///
    /// This is what the sessions screen's RUNNING and CPU columns read. A
    /// session's shell owns a pty and everything it runs inherits it, so once
    /// essh knows which pty is which session's — asked once, at connect — the
    /// columns cost nothing beyond the sweep that was already happening.
    ///
    /// Rolled up here rather than in the view because the full process list
    /// is thrown away after the top-N is taken, and re-fetching it per
    /// session per poll is the design this exists to avoid.
    pub by_tty: HashMap<String, TtySummary>,
}

/// What one pty's processes add up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TtySummary {
    /// Summed CPU across the pty's processes. The session's process tree, in
    /// the only sense the host can report cheaply.
    pub cpu_pct: f64,
    /// The foreground command: the leaf of the foreground process group.
    ///
    /// `ps` marks that group with `+` in STAT. The leaf rather than any
    /// member, because when a shell is waiting on `cargo build` both are
    /// foreground and only one of them is the answer to "what is it running".
    pub foreground: Option<String>,
    pub process_count: usize,
}

/// How a sweep ended. Three outcomes, and the middle one is the one the v2
/// handoff has no state for.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Answered, and the sweep parsed. A first sweep still lands here, with
    /// `cpu_pct: None`: memory, disk, load and processes need no previous
    /// reading, so the row fills in immediately rather than staying blank for
    /// a cycle, and the one figure that does need one says so by being
    /// absent.
    Read(Sample),
    /// Answers SSH, but the sweep produced nothing usable. A restricted
    /// shell, a forced command, a platform with no collectors. Reachable with
    /// no numbers is a fourth thing the hosts table has to say, and it is not
    /// the same as offline.
    Unreadable(String),
    /// Did not answer at all.
    Unreachable(String),
}

impl Outcome {
    fn is_failure(&self) -> bool {
        matches!(self, Outcome::Unreadable(_) | Outcome::Unreachable(_))
    }
}

/// What the sampler remembers about one host between sweeps.
#[derive(Debug)]
struct HostState {
    platform: Platform,
    /// The previous `/proc/stat`, which is what makes a CPU percentage
    /// possible at all. Held per host, so one host's first sweep does not
    /// poison another's.
    prev_cpu_raw: String,
    consecutive_failures: u32,
    next_due: Instant,
}

/// A sweep to run. The caller turns these into execs; the sampler never
/// opens a connection itself, which is what makes it testable without one.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    pub host: String,
    pub command: String,
}

pub struct FleetSampler {
    hosts: HashMap<String, HostState>,
    pub interval: Duration,
    /// How many sweeps may be in flight. The caller enforces it; the sampler
    /// states it, because the number belongs with the reason for it.
    pub concurrency: usize,
    pub process_limit: usize,
}

impl FleetSampler {
    pub fn new(interval: Duration, concurrency: usize, process_limit: usize) -> Self {
        Self {
            hosts: HashMap::new(),
            interval,
            concurrency,
            process_limit,
        }
    }

    /// A host's share of the interval, from its name. Deterministic, so the
    /// schedule is reproducible and a test can assert it.
    fn jitter_for(&self, host: &str) -> Duration {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in host.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let spread = self.interval.as_secs_f64() * JITTER;
        Duration::from_secs_f64(spread * (h % 1000) as f64 / 1000.0)
    }

    fn entry(&mut self, host: &str, now: Instant) -> &mut HostState {
        self.hosts
            .entry(host.to_string())
            .or_insert_with(|| HostState {
                platform: Platform::Undetected,
                prev_cpu_raw: String::new(),
                consecutive_failures: 0,
                // Due at once. A dashboard that takes a full interval to fill in
                // its first column is a dashboard people stop opening, and the
                // burst this allows happens once at startup rather than every
                // cycle — `concurrency` bounds it either way. The jitter is
                // applied from the first reschedule, which is where a repeating
                // burst would otherwise come from.
                next_due: now,
            })
    }

    /// The sweeps that are due, newest-known platform baked into each
    /// command, capped at `concurrency`.
    ///
    /// Returned in a stable order (due first, then by name) so a fleet larger
    /// than the concurrency limit rotates rather than starving its tail.
    pub fn due(&mut self, hosts: &[String], now: Instant) -> Vec<Sweep> {
        for host in hosts {
            self.entry(host, now);
        }
        let mut due: Vec<(&String, &HostState)> = self
            .hosts
            .iter()
            .filter(|(name, st)| hosts.contains(name) && st.next_due <= now)
            .collect();
        due.sort_by(|a, b| a.1.next_due.cmp(&b.1.next_due).then_with(|| a.0.cmp(b.0)));

        due.into_iter()
            .take(self.concurrency)
            .map(|(name, st)| Sweep {
                host: name.clone(),
                // An undetected platform still gets the Linux command: it
                // opens with `uname -s`, so one round trip both detects and
                // collects. Guessing wrong costs one wasted sweep; asking
                // first costs one round trip on every host, forever.
                command: st
                    .platform
                    .metrics_command(self.process_limit)
                    .unwrap_or_else(|| {
                        Platform::Linux
                            .metrics_command(self.process_limit)
                            .expect("linux always has a command")
                    }),
            })
            .collect()
    }

    /// Clear a host's back-off and make it due at once. SPEC §4.3's `r`.
    ///
    /// Without this, retrying an offline host would probe it and then wait up
    /// to `BACKOFF_INTERVAL` for its numbers to come back, so the key would
    /// look like it had half worked.
    pub fn retry_now(&mut self, host: &str, now: Instant) {
        let st = self.entry(host, now);
        st.consecutive_failures = 0;
        st.next_due = now;
    }

    /// Turn one sweep's raw output into an outcome, and reschedule the host.
    pub fn record(&mut self, host: &str, raw: Result<String, String>, now: Instant) -> Outcome {
        let outcome = match raw {
            Err(reason) => Outcome::Unreachable(reason),
            Ok(raw) => self.parse(host, &raw),
        };

        let interval = self.interval;
        let jitter = self.jitter_for(host);
        let st = self.entry(host, now);
        if outcome.is_failure() {
            st.consecutive_failures = st.consecutive_failures.saturating_add(1);
        } else {
            st.consecutive_failures = 0;
        }
        let step = if st.consecutive_failures >= BACKOFF_AFTER {
            BACKOFF_INTERVAL
        } else {
            interval
        };
        st.next_due = now + step + jitter;
        outcome
    }

    fn parse(&mut self, host: &str, raw: &str) -> Outcome {
        let sections = crate::monitor::collector::split_sections(raw);
        let Some(uname) = sections.get("UNAME") else {
            // No envelope at all: the command never ran. A restricted shell,
            // a forced command, an exec channel the server denied.
            return Outcome::Unreadable("host returned no metrics output".into());
        };
        let platform = Platform::from_uname(uname);
        if !platform.is_supported() {
            let reason = match &platform {
                Platform::Undetected => "could not run `uname -s`".to_string(),
                other => format!("no collectors for {}", other.label()),
            };
            let st = self.entry(host, Instant::now());
            st.platform = platform;
            return Outcome::Unreadable(reason);
        }

        let get = |k: &str| sections.get(k).cloned().unwrap_or_default();
        let cpu_raw = get("CPUSTAT");
        let prev = {
            let st = self.entry(host, Instant::now());
            st.platform = platform.clone();
            std::mem::replace(&mut st.prev_cpu_raw, cpu_raw.clone())
        };

        let (cpu_pct, cores) = match platform {
            Platform::Linux if cpu_raw.trim().is_empty() => (None, None),
            Platform::Linux => {
                // Differencing against itself yields 0%, which is why the
                // percentage is thrown away on the first sweep and only the
                // core count is kept. `prev` is empty exactly once per host.
                let (pct, per_core) = parser::parse_cpu(&cpu_raw, &prev);
                let pct = (!prev.is_empty()).then_some(pct);
                (pct, Some(per_core.len()))
            }
            _ => (
                parser::parse_macos_cpu(&cpu_raw),
                get("NCPU").trim().parse::<usize>().ok(),
            ),
        };

        let mut sample = Self::sample_from(&sections, self.process_limit);
        sample.cpu_pct = cpu_pct;
        sample.cores = sample.cores.or(cores);
        Outcome::Read(sample)
    }

    /// Everything except CPU, which needs the previous sweep.
    fn sample_from(sections: &HashMap<String, String>, process_limit: usize) -> Sample {
        let get = |k: &str| sections.get(k).cloned().unwrap_or_default();

        let mem_pct = {
            let (total, _free, available, _buffers, _cached) =
                parser::parse_meminfo(&get("MEMINFO"));
            if total > 0 {
                Some((total.saturating_sub(available) as f64 / total as f64) * 100.0)
            } else {
                parser::parse_vm_stat(&get("VMSTAT"), &get("MEMSIZE"))
                    .and_then(|(t, used, _)| (t > 0).then(|| (used as f64 / t as f64) * 100.0))
            }
        };

        // The busiest *user-visible* mount. `is_user_visible` already drops
        // the loop devices and overlay layers that would otherwise take the
        // maximum on any machine running containers.
        let disk_pct = parser::parse_df(&get("DF"))
            .iter()
            .filter(|d| d.is_user_visible())
            .map(|d| d.use_pct)
            .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |a: f64| a.max(p))));

        let load1 = {
            let raw = get("LOADAVG");
            if raw.trim().is_empty() {
                None
            } else if let Some((one, _, _)) = parser::parse_sysctl_loadavg(&raw) {
                Some(one)
            } else {
                Some(parser::parse_loadavg(&raw).0)
            }
        };

        // Parsed twice against one payload: the top-N the health screen
        // shows, and the per-pty rollup the sessions screen reads. The second
        // needs every process, not the busiest few, so it cannot be taken
        // from the first.
        let raw_ps = get("PS");
        Sample {
            cpu_pct: None,
            mem_pct,
            disk_pct,
            load1,
            cores: None,
            processes: parser::parse_ps(&raw_ps, process_limit),
            by_tty: roll_up_by_tty(&parser::parse_ps(&raw_ps, usize::MAX)),
        }
    }
}

/// Group a host's processes by controlling terminal.
///
/// The foreground pick is the part worth reading twice, and it took two goes.
///
/// `ps` marks every member of a terminal's foreground process group with `+`,
/// so a shell waiting on `cargo build` marks both. Taking the first reports
/// `-bash` for a host that is compiling, which is useless.
///
/// The first fix took the deepest one instead, and that is wrong the other
/// way: for a job that shells out, it reports the helper. A session running
/// `nightly-verify` reported `find`; a session running `cargo build` would
/// report `rustc`. The design is explicit about this — RUNNING says
/// `cargo build` and the busiest-processes panel says `rustc` — because they
/// answer different questions. RUNNING is *what you started*.
///
/// The rule that is actually right is simpler than either, and the reason is
/// the bit worth knowing: **a shell running a job is not in its own
/// foreground process group**. `bash` shows `Ss` while `cargo build` shows
/// `S+`. So the foreground set is the job and its descendants, and the one
/// the shell started is the lowest pid in it — a child is always started
/// after its parent. At a prompt the shell *is* the foreground group and is
/// the only member, so the same rule answers `-bash`.
pub fn roll_up_by_tty(procs: &[ProcessInfo]) -> HashMap<String, TtySummary> {
    let mut out: HashMap<String, TtySummary> = HashMap::new();
    for p in procs {
        if p.tty == "?" || p.tty.is_empty() {
            continue;
        }
        let entry = out.entry(p.tty.clone()).or_default();
        entry.cpu_pct += p.cpu_pct;
        entry.process_count += 1;
    }
    for (tty, summary) in out.iter_mut() {
        let group: Vec<&ProcessInfo> = procs
            .iter()
            .filter(|p| &p.tty == tty && p.is_foreground())
            .collect();
        summary.foreground = group.iter().min_by_key(|p| p.pid).map(|p| p.name.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampler() -> FleetSampler {
        FleetSampler::new(Duration::from_secs(5), 8, 10)
    }

    const LINUX_SWEEP: &str = "\
===UNAME===
Linux
===CPUSTAT===
cpu  1000 0 500 8000 0 0 0 0 0 0
cpu0 500 0 250 4000 0 0 0 0 0 0
cpu1 500 0 250 4000 0 0 0 0 0 0
===MEMINFO===
MemTotal:       65536000 kB
MemFree:        20000000 kB
MemAvailable:   35000000 kB
Buffers:         1000000 kB
Cached:          9000000 kB
===LOADAVG===
0.96 1.02 0.88 2/431 9182
===DF===
Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/sda1    100000000 81000000 19000000 81% /
===NETDEV===
Inter-|   Receive
===UPTIME===
1036800.00 8000000.00
===PS===
USER PID %CPU %MEM VSZ RSS TTY STAT START TIME COMMAND
matt 1 3.1 0.1 1000 1700 ? Ss 09:00 0:01 nginx: master
matt 40 0.5 0.1 1000 3600 pts/0 Ss 09:00 0:01 -bash
matt 41 61.0 4.0 9000 840000 pts/0 S+ 09:01 0:30 pg_dump events
matt 44 0.4 0.1 1000 2200 pts/0 R+ 09:01 0:02 gzip
matt 42 0.9 0.1 1000 3600 pts/1 Ss+ 09:00 0:01 -bash
===END===
";

    /// The second sweep. CPU has moved on, so a percentage is possible.
    fn second_sweep() -> String {
        LINUX_SWEEP.replace(
            "cpu  1000 0 500 8000 0 0 0 0 0 0",
            "cpu  1200 0 600 8100 0 0 0 0 0 0",
        )
    }

    /// SPEC §6.1: never a zero for a number we have not measured.
    ///
    /// A CPU percentage needs two `/proc/stat` reads to difference. The first
    /// sweep has one. v1's monitor published the 0 that falls out of that, and
    /// a fleet-wide table would publish eleven of them.
    #[test]
    fn the_first_sweep_reports_no_cpu_rather_than_zero() {
        let mut s = sampler();
        let now = Instant::now();
        let first = s.record("web-01", Ok(LINUX_SWEEP.into()), now);
        let Outcome::Read(sample) = first else {
            panic!("expected a reading, got {first:?}");
        };
        assert_eq!(sample.cpu_pct, None, "first sweep invented a CPU figure");
        // Everything that does not need differencing is there already, so the
        // row is not blank for a cycle.
        assert!(sample.mem_pct.is_some());
        assert_eq!(sample.disk_pct, Some(81.0));
        assert_eq!(sample.load1, Some(0.96));

        let second = s.record("web-01", Ok(second_sweep()), now);
        let Outcome::Read(sample) = second else {
            panic!("expected a reading");
        };
        assert!(sample.cpu_pct.is_some(), "second sweep still has no CPU");
    }

    /// One host's first sweep must not poison another's. The previous
    /// `/proc/stat` is per host, and sharing it would give host B a CPU
    /// percentage differenced against host A.
    #[test]
    fn cpu_history_does_not_leak_between_hosts() {
        let mut s = sampler();
        let now = Instant::now();
        s.record("web-01", Ok(LINUX_SWEEP.into()), now);
        let other = s.record("db-01", Ok(second_sweep()), now);
        let Outcome::Read(sample) = other else {
            panic!("expected a reading");
        };
        assert_eq!(
            sample.cpu_pct, None,
            "db-01 was given a percentage differenced against web-01"
        );
    }

    /// SPEC §1's disk rule alarms on the busiest mount, and the loop devices
    /// a container host is covered in are not mounts anyone can act on.
    #[test]
    fn disk_takes_the_busiest_real_mount() {
        let raw = LINUX_SWEEP.replace(
            "/dev/sda1    100000000 81000000 19000000 81% /",
            "/dev/sda1    100000000 81000000 19000000 81% /\n\
             /dev/loop3         1000     1000        0 100% /snap/core\n\
             tmpfs           1000000     4000   996000   1% /run",
        );
        let mut s = sampler();
        let now = Instant::now();
        s.record("web-01", Ok(raw.clone()), now);
        let Outcome::Read(sample) = s.record("web-01", Ok(raw), now) else {
            panic!("expected a reading");
        };
        assert_eq!(sample.disk_pct, Some(81.0));
    }

    /// A host that answers SSH and returns nothing usable is reachable with
    /// no numbers, which is not the same as offline and must not read as it.
    #[test]
    fn a_restricted_shell_is_unreadable_not_unreachable() {
        let mut s = sampler();
        let out = s.record(
            "locked-01",
            Ok("permission denied\n".into()),
            Instant::now(),
        );
        assert!(
            matches!(out, Outcome::Unreadable(ref why) if why.contains("no metrics output")),
            "got {out:?}"
        );
    }

    #[test]
    fn an_unsupported_platform_says_which_one() {
        let mut s = sampler();
        let raw = LINUX_SWEEP.replace("Linux", "FreeBSD");
        let out = s.record("bsd-01", Ok(raw), Instant::now());
        assert!(
            matches!(out, Outcome::Unreadable(ref why) if why.contains("FreeBSD")),
            "got {out:?}"
        );
    }

    /// SPEC §6.1 rule 5. A dead host must not be retried at the dashboard's
    /// rate: two hundred of them would be a reconnect storm every five
    /// seconds.
    #[test]
    fn a_host_that_keeps_failing_backs_off() {
        let mut s = sampler();
        let now = Instant::now();
        for _ in 0..BACKOFF_AFTER {
            s.record("gone-01", Err("connection refused".into()), now);
        }
        let due = s.due(&["gone-01".into()], now + Duration::from_secs(10));
        assert!(due.is_empty(), "a backed-off host was swept after 10s");
        let due = s.due(
            &["gone-01".into()],
            now + BACKOFF_INTERVAL + s.jitter_for("gone-01"),
        );
        assert_eq!(due.len(), 1, "a backed-off host never came back");
    }

    #[test]
    fn one_success_clears_the_back_off() {
        let mut s = sampler();
        let now = Instant::now();
        for _ in 0..BACKOFF_AFTER {
            s.record("flaky-01", Err("timeout".into()), now);
        }
        s.record("flaky-01", Ok(LINUX_SWEEP.into()), now);
        let next = now + s.interval + s.jitter_for("flaky-01");
        assert_eq!(s.due(&["flaky-01".into()], next).len(), 1);
    }

    /// SPEC §6.1 rule 3. Eleven hosts is nothing; two hundred simultaneous
    /// execs every five seconds is a denial of service you wrote yourself.
    #[test]
    fn a_sweep_never_exceeds_the_concurrency_limit() {
        let mut s = FleetSampler::new(Duration::from_secs(5), 8, 10);
        let hosts: Vec<String> = (0..200).map(|i| format!("host-{i:03}")).collect();
        let now = Instant::now() + Duration::from_secs(60);
        assert_eq!(s.due(&hosts, now).len(), 8);
    }

    /// And the tail still gets swept. Taking the first N by name would starve
    /// everything after `host-007` forever.
    #[test]
    fn a_fleet_larger_than_the_limit_rotates() {
        let mut s = FleetSampler::new(Duration::from_secs(5), 4, 10);
        let hosts: Vec<String> = (0..20).map(|i| format!("host-{i:02}")).collect();
        let mut swept: std::collections::HashSet<String> = Default::default();
        let mut now = Instant::now() + Duration::from_secs(60);
        for _ in 0..40 {
            for sweep in s.due(&hosts, now) {
                swept.insert(sweep.host.clone());
                s.record(&sweep.host, Ok(LINUX_SWEEP.into()), now);
            }
            now += Duration::from_secs(2);
        }
        assert_eq!(swept.len(), hosts.len(), "some hosts were never swept");
    }

    /// SPEC §4.3. `r` on an offline host has to bring its readings back, not
    /// just its dot — otherwise the key looks like it half worked.
    #[test]
    fn retry_clears_the_back_off_immediately() {
        let mut s = sampler();
        let now = Instant::now();
        for _ in 0..BACKOFF_AFTER {
            s.record("gone-01", Err("refused".into()), now);
        }
        assert!(s.due(&["gone-01".into()], now).is_empty());
        s.retry_now("gone-01", now);
        assert_eq!(s.due(&["gone-01".into()], now).len(), 1);
    }

    /// SPEC §6.2. A pty's processes are one session's process tree, and the
    /// foreground pick is what makes the column say `pg_dump` rather than
    /// `-bash` for a shell that is waiting on one — or `find`, which is what
    /// picking the deepest one gave for a job that shells out.
    #[test]
    fn processes_roll_up_per_terminal() {
        let mut s = sampler();
        let now = Instant::now();
        s.record("db-01", Ok(LINUX_SWEEP.into()), now);
        let Outcome::Read(sample) = s.record("db-01", Ok(second_sweep()), now) else {
            panic!("expected a reading");
        };

        // The process with no terminal belongs to no session and is absent.
        assert!(!sample.by_tty.contains_key("?"));
        assert_eq!(sample.by_tty.len(), 2);

        let busy = sample.by_tty.get("pts/0").expect("pts/0");
        assert!((busy.cpu_pct - 61.9).abs() < 0.01, "{:?}", busy.cpu_pct);
        assert_eq!(busy.process_count, 3);
        assert_eq!(
            busy.foreground.as_deref(),
            Some("pg_dump events"),
            "RUNNING is what the shell started, not the shell — which is not \
             even in the foreground group while a job runs — and not the \
             helper the job shelled out to"
        );

        let idle = sample.by_tty.get("pts/1").expect("pts/1");
        assert_eq!(idle.foreground.as_deref(), Some("-bash"));
        assert_eq!(idle.process_count, 1);
    }

    /// A shell at a prompt *is* its terminal's foreground group, so the same
    /// rule answers `-bash` there without a special case.
    #[test]
    fn a_shell_at_a_prompt_is_its_own_foreground_group() {
        let procs = vec![crate::monitor::ProcessInfo {
            pid: 42,
            name: "-bash".into(),
            cpu_pct: 0.0,
            mem_pct: 0.1,
            mem_rss_kb: 3600,
            state: "Ss+".into(),
            tty: "pts/1".into(),
        }];
        assert_eq!(
            roll_up_by_tty(&procs)["pts/1"].foreground.as_deref(),
            Some("-bash")
        );
    }

    /// A pty with nothing in the foreground has no answer, and says so rather
    /// than naming whichever process happened to be first.
    #[test]
    fn a_terminal_with_no_foreground_group_reports_none() {
        let procs = vec![crate::monitor::ProcessInfo {
            pid: 1,
            name: "-bash".into(),
            cpu_pct: 0.0,
            mem_pct: 0.0,
            mem_rss_kb: 0,
            state: "Ss".into(),
            tty: "pts/9".into(),
        }];
        let rolled = roll_up_by_tty(&procs);
        assert_eq!(rolled["pts/9"].foreground, None);
    }

    /// Hosts must not arrive at their sshd in one burst.
    #[test]
    fn due_times_are_spread_and_deterministic() {
        let s = sampler();
        let hosts = ["web-01", "web-02", "db-01", "cache-01", "edge-01"];
        let jitters: Vec<Duration> = hosts.iter().map(|h| s.jitter_for(h)).collect();
        assert!(
            jitters
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "every host got the same jitter"
        );
        for j in &jitters {
            assert!(*j < s.interval, "jitter exceeded the interval");
        }
        // Deterministic, so the schedule is reproducible.
        let again = sampler();
        for h in hosts {
            assert_eq!(s.jitter_for(h), again.jitter_for(h));
        }
    }

    /// A host added while essh is running fills its row on the next tick,
    /// not a full interval later.
    #[test]
    fn a_new_host_is_due_almost_immediately() {
        let mut s = sampler();
        let now = Instant::now();
        let due = s.due(&["new-01".into()], now + s.jitter_for("new-01"));
        assert_eq!(due.len(), 1);
    }

    /// The command carries `uname -s` in its first section, so one round trip
    /// both detects the platform and collects from it.
    #[test]
    fn an_undetected_host_still_gets_a_command() {
        let mut s = sampler();
        let due = s.due(
            &["unknown-01".into()],
            Instant::now() + Duration::from_secs(60),
        );
        assert_eq!(due.len(), 1);
        assert!(due[0].command.contains("===UNAME==="));
    }
}
