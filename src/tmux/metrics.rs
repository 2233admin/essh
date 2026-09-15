use super::*;
use std::{
    collections::BTreeMap,
    os::unix::process::CommandExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub details: super::details::Details,
    pub error: Option<String>,
    pub at: i64,
    pub running: Option<String>,
    pub rx: Option<f64>,
    pub tx: Option<f64>,
    pub rtt: Option<f64>,
    pub rtt_history: Vec<u64>,
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    pub disk: Option<f64>,
    pub peers: Vec<(String, f64, f64)>,
    pub rx_history: Vec<u64>,
    pub tx_history: Vec<u64>,
}
impl Snapshot {
    pub fn fresh(&self) -> bool {
        chrono::Utc::now().timestamp() - self.at < 12
    }
}
pub fn read(id: &str) -> Option<Snapshot> {
    if valid_id(id) && health_lease(id) {
        if let Ok(bytes) = fs::read(root().join(format!("{id}.health"))) {
            if let Ok(s) = serde_json::from_slice(&bytes) {
                return Some(s);
            }
        }
    }
    if !valid_id(id) {
        return None;
    }
    serde_json::from_slice(&fs::read(root().join(format!("{id}.metrics"))).ok()?).ok()
}
pub fn rate(value: Option<f64>) -> String {
    match value {
        None => "—".into(),
        Some(v) if v >= 1_000_000.0 => format!("{:.1}M", v / 1_000_000.0),
        Some(v) if v >= 1000.0 => format!("{:.1}k", v / 1000.0),
        Some(v) => format!("{v:.0}B"),
    }
}
const LIGHT: &str = "printf 'NET\\n'; cat /proc/net/dev 2>/dev/null";
const FULL: &str = "printf 'NET\\n'; cat /proc/net/dev 2>/dev/null; printf 'CPU\\n'; head -n 1 /proc/stat 2>/dev/null; printf 'MEM\\n'; cat /proc/meminfo 2>/dev/null; printf 'DISK\\n'; df -P / 2>/dev/null; printf 'PEERS\\n'; ss -tinH 2>/dev/null";

#[derive(Default, Debug)]
struct Counters {
    running: Option<String>,
    net: Option<(u64, u64)>,
    cpu: Option<(u64, u64)>,
    mem: Option<f64>,
    disk: Option<f64>,
    // Endpoint pairs identify sockets well enough for interval deltas; newly
    // appearing or reset sockets have no rate until a second sample exists.
    peers: BTreeMap<String, (String, u64, u64)>,
}
fn parse(raw: &str) -> Counters {
    let mut out = Counters::default();
    let mut section = "";
    let mut mem_total = 0.0;
    let mut mem_available = None;
    let mut socket: Option<(String, String)> = None;
    for line in raw.lines() {
        if [
            "NET",
            "CPU",
            "MEM",
            "DISK",
            "PEERS",
            "RUNNING",
            "OS",
            "UPTIME",
            "LOAD",
            "CORES",
            "FILESYSTEMS",
            "PROCESSES",
            "IO",
        ]
        .contains(&line)
        {
            section = line;
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        match section {
            "RUNNING" if !line.is_empty() => out.running = Some(safe_label(line)),
            "NET" => {
                if let Some((iface, data)) = line.split_once(':') {
                    if iface.trim() == "lo" {
                        continue;
                    }
                    let v: Vec<_> = data.split_whitespace().collect();
                    if let (Some(rx), Some(tx)) = (
                        v.first().and_then(|v| v.parse::<u64>().ok()),
                        v.get(8).and_then(|v| v.parse::<u64>().ok()),
                    ) {
                        let pair = out.net.get_or_insert((0, 0));
                        pair.0 = pair.0.saturating_add(rx);
                        pair.1 = pair.1.saturating_add(tx);
                    }
                }
            }
            "CPU" if fields.first() == Some(&"cpu") => {
                let nums: Vec<u64> = fields
                    .iter()
                    .skip(1)
                    .take(8)
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if nums.len() >= 4 {
                    out.cpu = Some((
                        nums.iter().sum(),
                        nums[3] + nums.get(4).copied().unwrap_or(0),
                    ));
                }
            }
            "MEM" if fields.len() >= 2 => {
                if fields[0] == "MemTotal:" {
                    mem_total = fields[1].parse().unwrap_or(0.0);
                }
                if fields[0] == "MemAvailable:" {
                    mem_available = fields[1].parse::<f64>().ok();
                }
            }
            "DISK" => {
                if let Some(pct) = fields.get(4).and_then(|v| v.strip_suffix('%')) {
                    out.disk = pct.parse().ok();
                }
            }
            "PEERS" => {
                if !line.starts_with(char::is_whitespace) && fields.len() >= 5 {
                    socket = Some((
                        format!("{}|{}", fields[3], fields[4]),
                        fields[4].to_string(),
                    ));
                } else if let Some((key, peer)) = &socket {
                    let bytes = |name: &str| {
                        fields
                            .iter()
                            .find_map(|v| v.strip_prefix(name))
                            .and_then(|v| v.parse::<u64>().ok())
                    };
                    if let (Some(rx), Some(tx)) = (bytes("bytes_received:"), bytes("bytes_sent:")) {
                        out.peers.insert(key.clone(), (peer.clone(), rx, tx));
                    }
                }
            }
            _ => (),
        }
    }
    if mem_total > 0.0 {
        out.mem = mem_available.map(|a| ((mem_total - a) / mem_total * 100.0).clamp(0.0, 100.0));
    }
    out
}
pub(super) fn delta(now: u64, old: u64, secs: f64) -> Option<f64> {
    if secs <= 0.0 {
        return None;
    }
    now.checked_sub(old).map(|d| d as f64 / secs)
}
fn sample(current: &Counters, old: &Counters, seconds: f64) -> Snapshot {
    let mut m = Snapshot {
        at: chrono::Utc::now().timestamp(),
        running: current.running.clone(),
        mem: current.mem,
        disk: current.disk,
        ..Default::default()
    };
    if let (Some((rx, tx)), Some((orx, otx))) = (current.net, old.net) {
        m.rx = delta(rx, orx, seconds);
        m.tx = delta(tx, otx, seconds);
    }
    if let (Some((total, idle)), Some((ot, oi))) = (current.cpu, old.cpu) {
        if let (Some(t), Some(i)) = (total.checked_sub(ot), idle.checked_sub(oi)) {
            if t > 0 && i <= t {
                m.cpu = Some((t - i) as f64 / t as f64 * 100.0);
            }
        }
    }
    let mut peers: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for (key, (peer, rx, tx)) in &current.peers {
        if let Some((_, orx, otx)) = old.peers.get(key) {
            if let (Some(r), Some(t)) = (delta(*rx, *orx, seconds), delta(*tx, *otx, seconds)) {
                let entry = peers.entry(peer.clone()).or_default();
                entry.0 += r;
                entry.1 += t;
            }
        }
    }
    m.peers = peers.into_iter().map(|(p, (r, t))| (p, r, t)).collect();
    m.peers.sort_by(|a, b| (b.1 + b.2).total_cmp(&(a.1 + a.2)));
    m.peers.truncate(4);
    m
}
fn push(history: &mut Vec<u64>, value: Option<f64>) {
    if let Some(v) = value {
        history.push(v as u64);
        if history.len() > 27 {
            history.remove(0);
        }
    }
}

pub fn spawn(
    s: SavedSession,
    window: String,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    spawn_sampler(s, window, stop, false)
}
fn spawn_sampler(
    s: SavedSession,
    window: String,
    stop: Arc<AtomicBool>,
    dashboard: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut old = Counters::default();
        let mut old_raw = String::new();
        let mut old_details = super::details::Details::default();
        let mut slow_at: Option<Instant> = None;
        let mut cpu_history = Vec::new();
        let mut mem_history = Vec::new();
        let mut previous = Instant::now();
        let mut rx_history = Vec::new();
        let mut tx_history = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            if dashboard && !health_lease(&s.id) {
                break;
            }
            if !dashboard && health_lease(&s.id) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let panes =
                call(&["list-panes", "-t", &window, "-F", "#{@essh_strip}"]).unwrap_or_default();
            let active = call(&[
                "display-message",
                "-p",
                "-t",
                &window,
                "#{window_active} #{session_attached}",
            ])
            .unwrap_or_default();
            let visible = (dashboard || panes.lines().any(|l| l == "1"))
                && active.starts_with("1 ")
                && !active.ends_with(" 0");
            if dashboard && !visible {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            // -O check and ProxyCommand=false prohibit telemetry from creating a
            // fresh authenticated connection when the interactive master is gone.
            let mut check = ssh_command(&s);
            check.args(["-O", "check", "--", destination(&s)]);
            if capture(check, Duration::from_secs(2)).is_ok() {
                let _ = option(&window, "@essh_state", "connected");
                let _ = option(&window, "@essh_dot", "#[fg=#35d07f]●#[default]");
                let start = Instant::now();
                let collect_slow =
                    dashboard && slow_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3));
                let detail_script = if dashboard {
                    format!(
                        "{FULL}; {}; {}",
                        super::details::SCRIPT,
                        if collect_slow {
                            super::details::SLOW
                        } else {
                            "true"
                        }
                    )
                } else {
                    if visible { FULL } else { LIGHT }.into()
                };
                let script = format!("{}; printf 'RUNNING\\n'; tmux -L essh display-message -p -t '=essh-{}:' '#{{pane_current_command}}' 2>/dev/null", detail_script, s.id);
                let script = format!("sh -c {}", quote(&script));
                let mut cmd = ssh_command(&s);
                cmd.args([
                    "-T",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ControlMaster=no",
                    "-o",
                    "ProxyCommand=false",
                    "--",
                    destination(&s),
                    &script,
                ]);
                if let Ok(raw) = capture(cmd, Duration::from_secs(3)) {
                    let counters = parse(&raw);
                    let mut detail =
                        super::details::parse(&raw, &old_raw, previous.elapsed().as_secs_f64());
                    if !collect_slow {
                        detail.disks = old_details.disks.clone();
                        detail.processes = old_details.processes.clone();
                    }
                    if collect_slow {
                        slow_at = Some(Instant::now());
                    }
                    for (name, rx, tx) in &detail.interfaces {
                        let mut hist = old_details
                            .interface_history
                            .get(name)
                            .cloned()
                            .unwrap_or_default();
                        push(&mut hist.0, *rx);
                        push(&mut hist.1, *tx);
                        detail.interface_history.insert(name.clone(), hist);
                    }
                    let mut m = sample(&counters, &old, previous.elapsed().as_secs_f64());
                    // RTT is measured separately by the host list's TCP probe;
                    // remote command execution time is not network latency.
                    m.rtt = ui::cached_latency(&s.host.name);
                    m.rtt_history = ui::cached_history(&s.host.name);
                    let _ = fs::write(
                        ui::probe_path(&s.host.name).with_extension("activity"),
                        serde_json::to_vec(&(m.at, m.running.clone())).unwrap_or_default(),
                    );
                    old_raw = raw;
                    push(&mut cpu_history, m.cpu);
                    push(&mut mem_history, m.mem);
                    detail.cpu_history = cpu_history.clone();
                    detail.mem_history = mem_history.clone();
                    old_details = detail.clone();
                    m.details = detail;
                    previous = start;
                    old = counters;
                    push(&mut rx_history, m.rx);
                    push(&mut tx_history, m.tx);
                    m.rx_history = rx_history.clone();
                    m.tx_history = tx_history.clone();
                    let path = root().join(format!(
                        "{}.{}",
                        s.id,
                        if dashboard { "health" } else { "metrics" }
                    ));
                    if let Ok(data) = serde_json::to_vec(&m) {
                        let temp = path.with_extension(if dashboard {
                            "health.tmp"
                        } else {
                            "metrics.tmp"
                        });
                        if fs::write(&temp, data).is_ok() {
                            let _ = fs::rename(temp, path);
                        }
                    }
                    let _ = option(&window, "@essh_state", "connected");
                    let _ = option(&window, "@essh_dot", "#[fg=#35d07f]●#[default]");
                }
            }
            let delay = if visible { 10 } else { 50 };
            for _ in 0..delay {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    })
}

fn health_lease(id: &str) -> bool {
    let prefix = format!("{id}.health-lease-");
    fs::read_dir(root())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name().to_string_lossy().starts_with(&prefix)
                && fs::read_to_string(e.path())
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok())
                    .is_some_and(|t| chrono::Utc::now().timestamp() - t < 4)
        })
}
pub struct HealthCollector {
    lease: PathBuf,
    id: String,
    window: String,
}
impl HealthCollector {
    pub fn new(w: &Window) -> Result<Self> {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let out = Self {
            lease: root().join(format!("{}.health-lease-{token}", w.id)),
            id: w.id.clone(),
            window: w.target.clone(),
        };
        fs::write(&out.lease, chrono::Utc::now().timestamp().to_string())?;
        out.ensure_worker()?;
        Ok(out)
    }
    fn ensure_worker(&self) -> Result<()> {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let lock = root().join(format!("{}.health-worker", self.id));
        let stale = fs::metadata(&lock)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|t| t > Duration::from_secs(10));
        if stale {
            let _ = fs::remove_file(&lock);
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                file.write_all(token.as_bytes())?;
                // The popup may close while a shared collector is serving another view.
                let result = Command::new(std::env::current_exe()?)
                    .process_group(0)
                    .args(["tmux", "health-sample", &self.id, &self.window, &token])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                match result {
                    Ok(mut child) => {
                        std::thread::spawn(move || {
                            let _ = child.wait();
                        });
                    }
                    Err(e) => {
                        let _ = fs::remove_file(lock);
                        return Err(e.into());
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    pub fn touch(&self) {
        let _ = fs::write(&self.lease, chrono::Utc::now().timestamp().to_string());
        let _ = self.ensure_worker();
    }
}
impl Drop for HealthCollector {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lease);
    }
}
pub fn health_worker(id: &str, window: &str, token: &str) -> Result<()> {
    let s = saved(id)?;
    let lock = root().join(format!("{id}.health-worker"));
    anyhow::ensure!(
        fs::read_to_string(&lock)? == token,
        "Health worker ownership changed"
    );
    let stop = Arc::new(AtomicBool::new(false));
    let worker = spawn_sampler(s, window.into(), stop.clone(), true);
    while !worker.is_finished() {
        if fs::read_to_string(&lock).ok().as_deref() != Some(token) {
            stop.store(true, Ordering::Relaxed);
            break;
        }
        let _ = fs::write(&lock, token);
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = worker.join();
    if fs::read_to_string(&lock).ok().as_deref() == Some(token) {
        let _ = fs::remove_file(lock);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_handle_counter_resets_and_unavailable_data() {
        let a=parse("NET\n eth0: 100 0 0 0 0 0 0 0 200\nCPU\ncpu 10 0 10 80 0 0 0 0\nMEM\nMemTotal: 1000 kB\nMemAvailable: 400 kB\nDISK\n/dev/a 100 10 90 10% /\n");
        let b = parse("NET\n eth0: 500 0 0 0 0 0 0 0 400\nCPU\ncpu 20 0 20 160 0 0 0 0\n");
        let m = sample(&b, &a, 2.0);
        assert_eq!(m.rx, Some(200.0));
        assert_eq!(m.tx, Some(100.0));
        assert_eq!(m.cpu, Some(20.0));
        assert_eq!(a.mem, Some(60.0));
        assert_eq!(a.disk, Some(10.0));
        assert_eq!(sample(&a, &b, 2.0).rx, None);
        assert_eq!(sample(&Counters::default(), &b, 2.0).mem, None);
    }
    #[test]
    fn socket_rates_use_interval_deltas_not_queue_sizes() {
        let a=parse("PEERS\nESTAB 0 0 10.0.0.1:10 10.0.0.2:20\n\t cubic bytes_sent:100 bytes_received:200\n");
        let b=parse("PEERS\nESTAB 0 0 10.0.0.1:10 10.0.0.2:20\n\t cubic bytes_sent:300 bytes_received:800\n");
        assert_eq!(
            sample(&b, &a, 2.0).peers,
            vec![("10.0.0.2:20".into(), 300.0, 100.0)]
        );
    }
}
