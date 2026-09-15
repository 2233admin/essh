use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    pub os: String,
    pub uptime: Option<f64>,
    pub load: Option<String>,
    pub mem_total: Option<u64>,
    pub mem_available: Option<u64>,
    pub mem_cached: Option<u64>,
    pub swap_total: Option<u64>,
    pub swap_free: Option<u64>,
    pub cores: Vec<f64>,
    pub disks: Vec<crate::monitor::DiskInfo>,
    pub processes: Vec<crate::monitor::ProcessInfo>,
    pub interfaces: Vec<(String, Option<f64>, Option<f64>)>,
    pub interface_history: BTreeMap<String, (Vec<u64>, Vec<u64>)>,
    pub io_read: Option<f64>,
    pub io_write: Option<f64>,
    pub cpu_history: Vec<u64>,
    pub mem_history: Vec<u64>,
}
// ps reports command names, not arguments (which can contain credentials).
pub const SCRIPT: &str = "printf 'OS\\n'; uname -sr 2>/dev/null; printf 'UPTIME\\n'; cat /proc/uptime 2>/dev/null; printf 'LOAD\\n'; cat /proc/loadavg 2>/dev/null; printf 'CORES\\n'; cat /proc/stat 2>/dev/null; printf 'IO\\n'; for d in /sys/block/*; do n=${d##*/}; case $n in loop*|ram*|dm-*|md*) continue;; esac; awk -v n=\"$n\" '$3==n {printf \"%.0f %.0f\\n\", $6*512, $10*512}' /proc/diskstats 2>/dev/null; done";
pub const SLOW: &str = "printf 'FILESYSTEMS\\n'; df -Pk 2>/dev/null; printf 'PROCESSES\\n'; ps -eo user,pid,pcpu,pmem,vsz,rss,tty,stat,start_time,time,comm --sort=-pcpu 2>/dev/null | head -n 101";
fn sections(raw: &str) -> BTreeMap<&str, String> {
    let mut out = BTreeMap::new();
    let mut name = "";
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
            name = line;
            continue;
        }
        let value = out.entry(name).or_insert_with(String::new);
        value.push_str(line);
        value.push('\n');
    }
    out
}
fn network(raw: &str) -> BTreeMap<String, (u64, u64)> {
    raw.lines()
        .filter_map(|l| {
            let (n, v) = l.split_once(':')?;
            let f: Vec<_> = v.split_whitespace().collect();
            Some((
                n.trim().into(),
                (f.first()?.parse().ok()?, f.get(8)?.parse().ok()?),
            ))
        })
        .collect()
}
fn io(raw: &str) -> Option<(u64, u64)> {
    let mut total = None;
    for l in raw.lines() {
        let f: Vec<_> = l.split_whitespace().collect();
        if f.len() == 2 {
            if let (Ok(r), Ok(w)) = (f[0].parse::<u64>(), f[1].parse::<u64>()) {
                let x = total.get_or_insert((0u64, 0u64));
                x.0 = x.0.saturating_add(r);
                x.1 = x.1.saturating_add(w);
            }
        }
    }
    total
}
pub fn parse(raw: &str, old: &str, seconds: f64) -> Details {
    let s = sections(raw);
    let prev = sections(old);
    let get = |key| s.get(key).map(String::as_str).unwrap_or("");
    let before = |key| prev.get(key).map(String::as_str).unwrap_or("");
    let memory = get("MEM");
    let field = |key: &str| {
        memory
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok())
    };
    let mut d = Details {
        os: get("OS").trim().into(),
        uptime: get("UPTIME")
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok()),
        load: (!get("LOAD").is_empty()).then(|| {
            get("LOAD")
                .split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join("  ")
        }),
        mem_total: field("MemTotal:"),
        mem_available: field("MemAvailable:"),
        mem_cached: field("Cached:"),
        swap_total: field("SwapTotal:"),
        swap_free: field("SwapFree:"),
        disks: crate::monitor::parser::parse_df(get("FILESYSTEMS"))
            .into_iter()
            .filter(|d| d.mount == "/" || d.is_user_visible())
            .collect(),
        processes: crate::monitor::parser::parse_ps(get("PROCESSES"), 100),
        ..Default::default()
    };
    if !before("CORES").is_empty() {
        d.cores = crate::monitor::parser::parse_cpu(get("CORES"), before("CORES")).1;
    }
    let oldnet = network(before("NET"));
    for (n, (r, w)) in network(get("NET")) {
        let previous = oldnet.get(&n);
        d.interfaces.push((
            n,
            previous.and_then(|p| metrics::delta(r, p.0, seconds)),
            previous.and_then(|p| metrics::delta(w, p.1, seconds)),
        ));
    }
    if let (Some((r, w)), Some((pr, pw))) = (io(get("IO")), io(before("IO"))) {
        d.io_read = metrics::delta(r, pr, seconds);
        d.io_write = metrics::delta(w, pw, seconds);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_interfaces_and_does_not_turn_counter_resets_into_rates() {
        let before =
            "NET\n eth0: 1000 0 0 0 0 0 0 0 2000\n lo: 50 0 0 0 0 0 0 0 50\nIO\n1000 2000\n";
        let now="NET\n eth0: 1200 0 0 0 0 0 0 0 2400\n lo: 10 0 0 0 0 0 0 0 10\nMEM\nMemTotal: 2000 kB\nMemAvailable: 500 kB\nSwapTotal: 1000 kB\nSwapFree: 800 kB\nIO\n1600 3000\n";
        let d = parse(now, before, 2.0);
        assert_eq!(d.interfaces[0], ("eth0".into(), Some(100.0), Some(200.0)));
        assert_eq!(d.interfaces[1], ("lo".into(), None, None));
        assert_eq!(d.io_read, Some(300.0));
        assert_eq!(d.io_write, Some(500.0));
        assert_eq!(d.mem_available, Some(500));
        assert_eq!(d.swap_free, Some(800));
        let missing = parse("OS\nDarwin\n", "", 1.0);
        assert!(missing.mem_total.is_none() && missing.io_read.is_none());
    }
}
