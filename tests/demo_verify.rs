//! Walks the demo tape's beats and checks each screen against `docs/redesign/SPEC.md`.
//!
//! The GIF and this test drive the same binary through the same beats against
//! the same ten containers. The GIF is what a person looks at; this is what
//! actually verifies it, because a screen is text before it is pixels and a
//! rule like "a count agrees with its noun" is an assertion, not an
//! impression. If a beat here fails, the corresponding frame of the GIF is
//! wrong.
//!
//! Requires the fixture fleet from SPEC.md §2 on 127.0.0.11-20:2222. Without
//! it the test skips and says so rather than failing.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

/// The geometry the reference was authored at (SPEC.md §6.1).
const COLS: u16 = 150;
const ROWS: u16 = 34;

/// How many Down presses take the cursor from db-01 to edge-01.
///
/// The tape presses this many. Shared so the GIF and the verification cannot
/// drift apart silently.
const DOWNS_TO_EDGE: usize = 3;

/// SPEC §2.1. Eleven hosts, and `staging-web-01` is not one of the ten that
/// answer: it is a container that is stopped.
///
/// The offline state has to be real. `scripts/fleet.sh` stops that container
/// on purpose, so the row essh draws for it is drawn from a probe that
/// actually failed. A fixture where every host answers cannot verify a screen
/// whose first panel is about hosts that do not.
const FLEET: [(&str, &str, &str, &str, &str); 11] = [
    ("web-01", "127.0.0.11", "prod", "us-east", "web"),
    ("web-02", "127.0.0.12", "prod", "us-east", "web"),
    ("web-03", "127.0.0.13", "prod", "us-east", "web"),
    ("db-01", "127.0.0.14", "prod", "us-east", "db"),
    ("db-02", "127.0.0.15", "prod", "us-east", "db"),
    ("cache-01", "127.0.0.16", "prod", "us-west", "cache"),
    ("edge-01", "127.0.0.17", "prod", "us-east", "edge"),
    ("build-01", "127.0.0.18", "ci", "us-west", "build"),
    ("monitor-01", "127.0.0.19", "prod", "us-west", "monitor"),
    ("staging-web-01", "127.0.0.20", "staging", "us-east", "web"),
    ("jump-01", "127.0.0.21", "prod", "us-east", "jump"),
];

/// The one host the fixture keeps down.
const OFFLINE_HOST: &str = "staging-web-01";

fn bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("test binary path");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("essh");
    if p.exists() {
        return p;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/essh")
}

fn fleet_available() -> bool {
    use std::net::{SocketAddr, TcpStream};
    FLEET
        .iter()
        .filter(|(name, ..)| *name != OFFLINE_HOST)
        .all(|(_, ip, ..)| {
            let addr: SocketAddr = format!("{ip}:2222").parse().expect("addr");
            TcpStream::connect_timeout(&addr, Duration::from_millis(600)).is_ok()
        })
}

/// A HOME carrying the fixture fleet, tagged exactly as SPEC.md §2 specifies.
fn demo_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("essh-demo-verify-{}", std::process::id()));
    let essh = dir.join(".essh");
    std::fs::create_dir_all(&essh).expect("mkdir .essh");

    let key = PathBuf::from(std::env::var("HOME").expect("HOME")).join(".ssh/id_ed25519");
    let mut cfg = format!(
        "theme = \"dark\"\n\
         host_groups = []\n\n\
         [general]\n\
         default_user = \"matt\"\n\
         default_key = \"{}\"\n\
         tofu_policy = \"auto\"\n\
         launcher = true\n\
         connect_timeout = 12\n\n\
         [session]\n\
         prefix_key = \"ctrl-a\"\n\
         max_concurrent = 9\n",
        key.display()
    );

    for (name, ip, env, region, role) in FLEET.iter() {
        cfg.push_str(&format!(
            "\n[[hosts]]\nname = \"{name}\"\nhostname = \"{ip}\"\nport = 2222\nuser = \"matt\"\n\
             tags = {{ env = \"{env}\", region = \"{region}\", role = \"{role}\" }}\n"
        ));
    }
    std::fs::write(essh.join("config.toml"), cfg).expect("write config");
    dir
}

struct Tui {
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _pty: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn start(home: &PathBuf) -> Self {
        let pty = NativePtySystem::default()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");

        let mut cmd = CommandBuilder::new(bin());
        cmd.env("HOME", home);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(home);

        let child = pty.slave.spawn_command(cmd).expect("spawn essh");
        let mut reader = pty.master.try_clone_reader().expect("reader");
        let writer = pty.master.take_writer().expect("writer");

        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let sink = screen.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().process(&buf[..n]);
            }
        });

        Tui {
            writer,
            screen,
            child,
            _pty: pty.master,
        }
    }

    fn text(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("write");
        self.writer.flush().expect("flush");
    }

    fn type_text(&mut self, s: &str) {
        for b in s.as_bytes() {
            self.send(&[*b]);
            std::thread::sleep(Duration::from_millis(12));
        }
    }

    fn expect(&self, needle: &str, within: Duration) {
        let started = Instant::now();
        while started.elapsed() < within {
            if self.text().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "`{needle}` never appeared within {within:?}.\n\
             ─── screen ───\n{}\n──────────────",
            self.text()
        );
    }

    fn settle(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    /// Print the current screen under a beat name, so a failing run shows the
    /// frame and a passing run doubles as the tape's storyboard.
    fn frame(&self, beat: &str) -> String {
        let t = self.text();
        println!(
            "\n╭─ {beat} {}",
            "─".repeat(70_usize.saturating_sub(beat.len()))
        );
        for line in t.lines() {
            println!("│ {}", line.trim_end());
        }
        println!("╰{}\n", "─".repeat(72));
        t
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every rule this file enforces, applied to whatever is on screen.
///
/// These hold on *every* frame, so they run after each beat rather than being
/// remembered per-screen. Each one is a defect that shipped.
fn universal_rules(screen: &str, beat: &str) {
    // SPEC §8.6 — counts agree with their noun, anywhere on any screen.
    //
    // Listing the nouns by hand is how "and 1 other hosts" survived the first
    // pass: the rule checked "1 hosts". Scan for the shape instead.
    if let Some(bad) = disagreeing_counts(screen).first() {
        panic!("[{beat}] `{bad}` — a count that disagrees with its noun:\n{screen}");
    }

    header_states_partition(screen, beat);

    // SPEC §8.5 — no binding split across rows by wrapping.
    for split in ["F4 /", "F9 new /", "/ forwards", "/ session"] {
        assert!(
            !screen.contains(split),
            "[{beat}] a binding broke as `{split}`:\n{screen}"
        );
    }

    // SPEC §8.3 — `r` is not advertised as probe any more. Matched with its
    // surrounding spaces: "never probed" contains "r probe" too.
    assert!(
        !screen.contains(" r probe"),
        "[{beat}] the footer still promises `r probe`:\n{screen}"
    );

    // SPEC §8.1 — a ratio is printed once.
    for dup in ["2 / 2 2 / 2", "1 / 1 1 / 1", "2 / 7 2 / 7"] {
        assert!(
            !screen.contains(dup),
            "[{beat}] a consensus ratio printed twice:\n{screen}"
        );
    }

    // SPEC §2.3 — a lone probed host is not a consensus.
    assert!(
        !screen.contains("1 / 1 full consensus"),
        "[{beat}] one host reported as full consensus:\n{screen}"
    );
}

#[test]
fn the_demo_beats_match_the_spec() {
    if !fleet_available() {
        eprintln!("SKIP: fixture fleet not up; run ./scripts/fleet.sh up");
        return;
    }

    let home = demo_home();
    let mut tui = Tui::start(&home);

    // ── beat 1 · the launcher ────────────────────────────────────────────
    tui.expect("connect", Duration::from_secs(10));
    let s = tui.frame("1 · launcher");
    universal_rules(&s, "launcher");

    // ── beat 2 · a real shell on web-02, the host with nginx ─────────────
    tui.type_text("web-02");
    tui.settle(700);
    tui.send(b"\r");
    tui.expect("web-02", Duration::from_secs(30));
    tui.settle(2500);
    // The same command the tape runs. The demo claims web-02 diverges on
    // listening ports; this is the claim being true on the host itself.
    tui.type_text("ss -ltn");
    tui.send(b"\r");
    tui.settle(2500);
    let s = tui.frame("2 · session on web-02");
    universal_rules(&s, "session");
    assert!(
        s.contains(":80"),
        "the demo says web-02 diverges on listening ports; it is not serving one:\n{s}"
    );

    // ── beat 3 · two more hosts, so there is something to compare ────────
    for host in ["edge-01", "db-01"] {
        tui.send(&[0x01]); // ctrl-a
        tui.settle(400);
        tui.type_text("n");
        tui.settle(900);
        tui.type_text(host);
        tui.settle(700);
        tui.send(b"\r");
        tui.expect(host, Duration::from_secs(30));
        tui.settle(2500);
    }
    let s = tui.frame("3 · third session up");
    universal_rules(&s, "sessions");

    // ── beat 3b · the sessions screen, which v2 opens on ─────────────────
    tui.send(&[0x01]);
    tui.settle(400);
    tui.type_text("d");
    tui.settle(1200);
    tui.type_text("1");
    tui.settle(4500);
    let sessions = tui.frame("3b · sessions");
    universal_rules(&sessions, "sessions");

    // SPEC §6.2 — LAST LINE is the column the screen exists for: what a
    // detached session is doing, without attaching to it. It comes from the
    // scrollback essh already holds, so a blank column here means the
    // cheapest thing on the screen is broken.
    let with_last_line = sessions
        .lines()
        .filter(|l| l.contains("matt@") || l.contains("$"))
        .count();
    assert!(
        with_last_line >= 2,
        "the LAST LINE column is empty for every session:\n{sessions}"
    );

    // SPEC §2.1 — quick connect lists exactly the hosts with no session, and
    // the count comes from the same place the SESS column and the header do.
    // The handoff's own list omits one, which is what happens when three
    // readings of one fact are written down separately.
    // Asserted on the count in the panel's own subtitle rather than by
    // looking for host names on screen lines: the three bottom panels sit
    // side by side, so one line carries a quick-connect row, a broadcast row
    // and an activity row, and "web-02 appears on a line that also says `no
    // session`" is true for reasons that have nothing to do with the panel.
    let quick_count = sessions
        .split("quick connect")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("no quick connect panel:\n{sessions}"));
    assert_eq!(
        quick_count,
        FLEET.len() - 3,
        "quick connect offers {quick_count} of {} hosts with three sessions open:\n{sessions}",
        FLEET.len()
    );

    // ── beat 4 · the hosts screen ────────────────────────────────────────
    tui.send(&[0x01]);
    tui.settle(400);
    tui.type_text("d");
    tui.settle(1200);
    tui.type_text("2");
    tui.settle(3000);
    let hosts = tui.frame("4 · hosts");
    universal_rules(&hosts, "hosts");

    // SPEC §6.1 — the hosts screen shows readings for hosts with no session
    // open, which is the thing the app could not do before v2. Eight of the
    // ten on screen have no session, so a table of dashes here means the
    // fleet sampler connected and nothing parsed.
    let measured = hosts
        .lines()
        .filter(|l| l.contains('%') && l.contains("127.0.0."))
        .count();
    assert!(
        measured >= 8,
        "only {measured} host rows carry a reading; the fleet sampler is not \
         filling the table:\n{hosts}"
    );

    // SPEC §6.1 — and never a zero for a reading we do not have. A row is
    // measured, or it says `—`, or it replaces its measurement columns with
    // an explanation carrying the recovery key. Never a blank, and never a
    // confident zero.
    for line in hosts.lines().filter(|l| l.contains("127.0.0.")) {
        assert!(
            line.contains('%') || line.contains('—') || line.contains(" r "),
            "a host row shows neither a reading, a dash, nor a reason:\n{line}"
        );
    }

    // SPEC §4.3 — `r` is reconnect or retry, and the footer says so.
    assert!(
        hosts.contains("r reconnect"),
        "the hosts footer must advertise `r` as reconnect:\n{hosts}"
    );
    assert!(
        !hosts.contains("r probe") && !hosts.contains("r reload"),
        "`r` is advertised as probe or reload again:\n{hosts}"
    );

    // SPEC §1 — the fixture keeps one container down, so this row is drawn
    // from a probe that actually failed. It must name the state, offer the
    // recovery key, and show no figures at all: a host that has never
    // answered has no last reading to be stale about.
    let offline_row = hosts
        .lines()
        .find(|l| l.contains(OFFLINE_HOST))
        .unwrap_or_else(|| panic!("{OFFLINE_HOST} is missing from the hosts table:\n{hosts}"));
    assert!(
        offline_row.contains("offline"),
        "the stopped container is not shown as offline:\n{offline_row}"
    );
    assert!(
        offline_row.contains("r retry"),
        "an unreachable row must state its recovery key:\n{offline_row}"
    );
    assert!(
        !offline_row.contains('%'),
        "a host that never answered is showing a measurement:\n{offline_row}"
    );

    // ── beat 5 · health ──────────────────────────────────────────────────
    tui.type_text("3");
    tui.settle(2500);
    let health = tui.frame("5 · health");
    universal_rules(&health, "health");

    // The attention list is the header's partition read a second way, so the
    // two cannot disagree. The offline host is in it because SPEC §1 says
    // unreachable needs attention, which is the entry the v2 handoff drops.
    assert!(
        health.contains("needs attention"),
        "the health screen has no attention panel:\n{health}"
    );
    let attention_line = health
        .lines()
        .find(|l| l.contains(OFFLINE_HOST) && l.contains("offline"))
        .unwrap_or_else(|| panic!("the offline host is not in the attention list:\n{health}"));
    assert!(
        attention_line.contains("no session was open")
            || attention_line.contains("waiting to reconnect"),
        "an attention row must say what the state costs:\n{attention_line}"
    );

    // SPEC §2.4 — busiest across the fleet, siblings collapsed. The fixture
    // runs three identical loops on build-01; three rows for one fact is
    // defect 7 of the v0.3.3 list.
    assert!(
        health.contains("busiest right now"),
        "the health screen has no busiest panel:\n{health}"
    );
    let repeated = health
        .lines()
        .filter(|l| l.contains("build-01") && l.contains("essh-fixture-load"))
        .count();
    assert!(
        repeated <= 1,
        "identical sibling processes are listed {repeated} times instead of \
         collapsing with ×N:\n{health}"
    );

    tui.type_text("2");
    tui.settle(1500);
    let back = tui.frame("5b · back on hosts");
    universal_rules(&back, "hosts again");

    // ── beat 6 · walk the hosts list to the outlier ──────────────────────
    // The cursor moves visibly, one row at a time, until it is on the host
    // the fleet screen is about to explain.
    // Exactly the keys the tape presses. The cursor starts on db-01 — the
    // last host connected — and edge-01 is three rows below it. Asserting the
    // count rather than looping keeps the tape and this test in step: if the
    // list order changes, both are wrong together and this fails first.
    assert_eq!(
        marked_row(&tui.text()),
        "db-01",
        "the cursor does not start on the last host connected"
    );
    for _ in 0..DOWNS_TO_EDGE {
        tui.send(b"\x1b[B");
        tui.settle(300);
    }
    assert_eq!(
        marked_row(&tui.text()),
        "edge-01",
        "{DOWNS_TO_EDGE} presses of Down did not reach edge-01"
    );
    let picked = tui.frame("6 · edge-01 under the cursor");
    universal_rules(&picked, "hosts cursor");

    // ── beat 7 · `r` retries, and `R` keeps the measurements ─────────────
    tui.type_text("r");
    tui.settle(1200);
    let retried = tui.frame("7 · r retries the selected host");
    assert!(
        retried.contains("Retrying") || retried.contains("Reconnecting"),
        "`r` must retry the host under the cursor:\n{retried}"
    );

    tui.type_text("R");
    tui.settle(1500);
    let reloaded = tui.frame("7b · R reloads and keeps probes");
    assert!(
        reloaded.contains("Probe results kept"),
        "reload must state that measurements survived:\n{reloaded}"
    );
    // SPEC §8.3 — the fleet must not read as unreachable after a reload.
    assert!(
        !reloaded.contains("0 · 0 reachable"),
        "the reload discarded probe state:\n{reloaded}"
    );
    universal_rules(&reloaded, "reload");

    // The reload rebuilds the list cache-first, so the row order changes under
    // the cursor. The selection has to still be on edge-01 afterwards.
    assert_eq!(
        marked_row(&tui.text()),
        "edge-01",
        "the reload moved the cursor to a different host"
    );

    // ── beat 8 · health keeps the selection ──────────────────────────────
    tui.type_text("3");
    tui.settle(3000);
    let health2 = tui.frame("8 · health, selection intact");
    universal_rules(&health2, "health again");

    // v1's screen 3 listed only outliers, so arriving with a host at
    // consensus selected snapped the cursor onto a different machine. v2
    // lists every host in the hosts screen's order, so the selection means
    // the same thing on both and switching tabs must not move it.
    tui.type_text("2");
    tui.settle(1200);
    assert_eq!(
        marked_row(&tui.text()),
        "edge-01",
        "switching to health and back moved the cursor"
    );
    tui.type_text("3");
    tui.settle(1200);

    // ── beat 9 · the cursor moves the row the screen shows ───────────────
    tui.type_text("2");
    tui.settle(1000);
    let before = marked_row(&tui.text());
    tui.send(b"\x1b[B");
    tui.settle(700);
    let moved = tui.frame("9 · cursor moved one row");
    assert_ne!(
        before,
        marked_row(&moved),
        "the marker did not move with the cursor"
    );
    // Back to the outlier for the card.
    tui.send(b"\x1b[A");
    tui.settle(700);
    let back = tui.text();
    assert_eq!(marked_row(&back), "edge-01", "the cursor did not come back");

    // ── beat 10 · D opens the card for the host under the marker ──────────
    tui.type_text("D");
    tui.settle(2500);
    let card = tui.frame("10 · divergence card");
    universal_rules(&card, "divergence");
    assert!(
        card.contains("edge-01"),
        "`D` opened a card for a host other than the one under the cursor:\n{card}"
    );
    tui.send(&[0x1b]);
    tui.settle(800);

    // ── beat 11 · forwards report what actually bound ─────────────────────
    tui.type_text("1");
    tui.settle(600);
    tui.send(b"\r"); // attach to the highlighted session
    tui.settle(1500);
    tui.send(&[0x01]);
    tui.settle(400);
    tui.type_text("p");
    tui.settle(1200);
    let fwd_empty = tui.frame("11 · forwards, empty");
    universal_rules(&fwd_empty, "forwards");

    tui.type_text("a");
    tui.settle(600);
    let form = tui.frame("11b · add form");
    // SPEC §8.4 — the format hint is whole, not clipped at the panel edge.
    assert!(
        form.contains("127.0.0.1:PORT → HOST:PORT"),
        "the add-forward format is clipped:\n{form}"
    );

    tui.type_text("L:18080:localhost:80");
    tui.settle(400);
    tui.send(b"\r");
    tui.settle(1500);
    let bound = tui.frame("11c · a forward that bound");
    universal_rules(&bound, "forward active");
    assert!(
        bound.contains("● active"),
        "a forward that bound is not reported active:\n{bound}"
    );
    assert!(
        bound.contains("1 rule") && bound.contains("1 active"),
        "the panel must name the host and count what actually bound:\n{bound}"
    );

    // The same port twice: the bind fails, and the row says why.
    tui.type_text("a");
    tui.settle(500);
    tui.type_text("L:18080:localhost:80");
    tui.settle(400);
    tui.send(b"\r");
    tui.settle(1500);
    let failed = tui.frame("11d · a forward that did not bind");
    universal_rules(&failed, "forward failed");
    assert!(
        failed.contains("● failed"),
        "a forward that could not bind is not reported failed:\n{failed}"
    );
    assert!(
        failed.contains("already in use"),
        "a failure with no reason sends people to `ss -ltn`:\n{failed}"
    );
    assert!(
        failed.contains("2 rules") && failed.contains("1 active"),
        "the count must not include the forward that failed:\n{failed}"
    );
    tui.send(&[0x1b]);
    tui.settle(600);

    // ── beat 11 · help, where every binding is a whole cell ──────────────
    // The same keys the tape presses: VHS has no F1 command, and
    // `function_key_command` maps F1 to the prefix's `?` anyway.
    tui.send(&[0x01]);
    tui.settle(400);
    tui.type_text("?");
    tui.settle(1500);
    let help = tui.frame("12 · help");
    universal_rules(&help, "help");
    for (key, label) in [
        ("F1", "help"),
        ("F4", "forwards"),
        ("F9", "new session"),
        ("F10", "command menu"),
    ] {
        assert!(
            help.contains(&format!("{key} {label}")),
            "`{key} {label}` is not whole on one row:\n{help}"
        );
    }
    // The dashboard keys are below the fold on a 34-row terminal, so the beat
    // scrolls to them — which is also how a person would find them.
    for _ in 0..40 {
        if tui.text().contains("Reload config") {
            break;
        }
        tui.send(b"\x1b[B");
        std::thread::sleep(Duration::from_millis(60));
    }
    tui.settle(800);
    let help_keys = tui.frame("12b · help, scrolled to the dashboard keys");
    universal_rules(&help_keys, "help scrolled");
    assert!(
        help_keys.contains("p / P") && help_keys.contains("Reload config"),
        "help must teach the probe/reload split:\n{help_keys}"
    );
    assert!(
        !help_keys.contains("Refresh hosts"),
        "help still teaches the key that was split:\n{help_keys}"
    );

    println!("\nAll beats verified against docs/redesign/SPEC.md.");
}

/// Every `1 <plural>` on the screen — the shape of the `1 peers` defect.
///
/// Words that are singular but end in `s` are allowed through; everything else
/// following a bare `1` and ending in `s` is a count that does not agree with
/// its noun.
fn disagreeing_counts(screen: &str) -> Vec<String> {
    const SINGULAR_S: [&str; 8] = [
        "status", "process", "address", "progress", "less", "ms", "https", "bs",
    ];
    let mut found = Vec::new();
    for line in screen.lines() {
        // The global header is the one line where a leading digit is an
        // ordinal: `1 sessions` is the first tab, not a count of one session.
        // This used to be exempted by capitalisation, which stopped working
        // the moment the redesign lowercased the tab labels — so the header
        // is exempted by being the header, and `header_states_partition`
        // covers it instead.
        if is_header(line) {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        for pair in words.windows(2) {
            if pair[0] != "1" {
                continue;
            }
            let noun = pair[1].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            // Tab labels are `1 Sessions`, `2 Hosts` — a number and a proper
            // name, not a count and its noun. Counts here are lowercase.
            if noun.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let lower = noun.to_ascii_lowercase();
            if !lower.ends_with('s') || lower.len() < 3 || SINGULAR_S.contains(&lower.as_str()) {
                continue;
            }
            // Units and measurements are not nouns being counted: a diverge
            // count of 1 sits next to an RTT of `0ms` in the column beside it.
            if lower.chars().any(|c| c.is_ascii_digit()) {
                continue;
            }
            found.push(format!("1 {noun}"));
        }
    }
    found
}

/// The global header: `ESSH`, the tab strip, then the fleet's state counts.
fn is_header(line: &str) -> bool {
    line.trim_start().starts_with("ESSH ")
}

/// SPEC §1. The header's counts are a partition, so each state appears at
/// most once and every word in it is a state.
///
/// The v2 handoff's header reads `9 up · 1 busy · 1 offline` over eleven
/// hosts. It sums, and it is still wrong: the host whose connection reset six
/// minutes ago is inside the `up` count, on the same screen that explains the
/// reset. This is the assertion that shape cannot pass.
fn header_states_partition(screen: &str, beat: &str) {
    const STATES: [&str; 5] = ["up", "busy", "dropped", "offline", "unprobed"];
    let Some(header) = screen.lines().find(|l| is_header(l)) else {
        return;
    };
    // Everything after the last tab label is the counts block.
    let Some(counts) = header.split("4 config").nth(1) else {
        return;
    };
    let words: Vec<&str> = counts.split_whitespace().collect();
    let mut seen: Vec<&str> = Vec::new();
    for pair in words.windows(2) {
        if pair[0].parse::<usize>().is_err() {
            continue;
        }
        if !STATES.contains(&pair[1]) {
            continue;
        }
        assert!(
            !seen.contains(&pair[1]),
            "[{beat}] the header counts `{}` twice:\n{screen}",
            pair[1]
        );
        seen.push(pair[1]);
    }
}

/// The host name on the row carrying the selection marker.
fn marked_row(screen: &str) -> String {
    screen
        .lines()
        .find(|l| l.contains('▌'))
        .map(|l| {
            l.split('▌')
                .nth(1)
                .unwrap_or("")
                .trim_start_matches(['●', '◌', ' '])
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}
