//! Local tmux owns navigation; an isolated remote tmux session owns each shell.
mod details;
mod metrics;
mod navigation;
mod ui;

use crate::config::AppConfig;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(clap::Subcommand, Debug)]
pub enum Action {
    /// Start or reattach the persistent workspace
    Start,
    /// Reload navigation without stopping connections
    Refresh,
    #[command(hide = true)]
    HealthSample {
        id: String,
        window: String,
        token: String,
    },
    #[command(hide = true)]
    Navigate { command: String },
    #[command(hide = true)]
    View { view: navigation::View },
    /// Host list (normally opened by the workspace)
    List {
        #[arg(long)]
        new: bool,
    },
    /// Session switcher
    Switch,
    /// Keyboard reference
    Help,
    /// Open a configured host; --new opens an independent shell
    Open {
        host: String,
        #[arg(long)]
        new: bool,
    },
    #[command(hide = true)]
    Attach {
        id: String,
        #[arg(long)]
        wait: bool,
    },
    #[command(hide = true)]
    Toggle { window: String },
    #[command(hide = true)]
    Strip {
        id: String,
        #[arg(long)]
        half: bool,
    },
    #[command(hide = true)]
    SyncSide { window: String },
    #[command(hide = true)]
    Status { window: String },
    #[command(hide = true)]
    Kill { window: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub user: Option<String>,
    pub key: Option<String>,
    pub jump: Option<String>,
    pub tags: String,
    pub via_ssh_config: bool,
    pub proxy: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub host: Host,
    pub created: i64,
    #[serde(default)]
    pub index: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub target: String,
    pub index: String,
    pub id: String,
    pub host: String,
    pub active: bool,
    pub state: String,
}

pub fn run(action: Action, config: AppConfig) -> Result<()> {
    match action {
        Action::Start => start(),
        Action::Refresh => {
            configure()?;
            for w in windows()? {
                let mut record = saved(&w.id)?;
                record.index = w.index.parse().ok();
                fs::write(session_path(&record.id)?, serde_json::to_vec(&record)?)?;
            }
            call(&["respawn-pane", "-k", "-t", "essh:0", &helper(&["list"])?])?;
            Ok(())
        }
        Action::HealthSample { id, window, token } => metrics::health_worker(&id, &window, &token),
        Action::Navigate { command } => navigation::run(&command),
        Action::View { view } => ui::workspace::run(config, view, true, false),
        Action::List { new } => ui::hosts(config, new),
        Action::Switch => ui::switcher(),
        Action::Help => ui::help(),
        Action::Open { host, new } => {
            let host = hosts(&config)
                .into_iter()
                .find(|h| h.name == host)
                .context("Host not found in SSH or essh configuration")?;
            open(&host, new)
        }
        Action::Attach { id, wait } => attach(&id, wait),
        Action::Toggle { window } => toggle(&window),
        Action::Strip { id, half } => {
            if half {
                ui::workspace::half(&id)
            } else {
                ui::strip(&id)
            }
        }
        Action::SyncSide { window } => sync_side(&window),
        Action::Status { window } => {
            print_status(&window);
            Ok(())
        }
        Action::Kill { window } => kill(&window),
    }
}

pub fn hosts(config: &AppConfig) -> Vec<Host> {
    let ssh = crate::sshconfig::SshConfig::load(&crate::sshconfig::default_path());
    let local_user = std::env::var("USER").unwrap_or_default();
    let mut hosts: Vec<_> = ssh
        .aliases()
        .into_iter()
        .map(|name| {
            let r = ssh.resolve(&name, &local_user);
            Host {
                name,
                hostname: r.hostname,
                port: r.port,
                user: r.user,
                key: None,
                jump: None,
                tags: String::new(),
                via_ssh_config: true,
                proxy: r.proxy_command.is_some() || r.proxy_jump.is_some(),
            }
        })
        .collect();
    for h in &config.hosts {
        hosts.retain(|entry| entry.name != h.name);
        let r = ssh.resolve(&h.hostname, &local_user);
        hosts.push(Host {
            name: h.name.clone(),
            hostname: h.hostname.clone(),
            port: h.port,
            user: h.user.clone().or(config.general.default_user.clone()),
            key: h.key.clone().or(config.general.default_key.clone()),
            jump: h.jump_host.clone(),
            tags: h
                .tags
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" "),
            via_ssh_config: false,
            proxy: h.jump_host.is_some() || r.proxy_command.is_some() || r.proxy_jump.is_some(),
        });
    }
    if let Ok(cached) = crate::cache::CacheDb::open_default().and_then(|db| db.list_hosts()) {
        for h in cached {
            if hosts
                .iter()
                .any(|entry| entry.hostname == h.hostname && entry.port == h.port)
            {
                continue;
            }
            let name = if h.port == 22 {
                h.hostname.clone()
            } else {
                format!("{}:{}", h.hostname, h.port)
            };
            if hosts.iter().any(|entry| entry.name == name) {
                continue;
            }
            let r = ssh.resolve(&h.hostname, &local_user);
            hosts.push(Host {
                name,
                hostname: h.hostname,
                port: h.port,
                user: config.general.default_user.clone(),
                key: config.general.default_key.clone(),
                jump: None,
                tags: h
                    .tags
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                via_ssh_config: false,
                proxy: r.proxy_command.is_some() || r.proxy_jump.is_some(),
            });
        }
    }
    hosts
}

fn root() -> PathBuf {
    AppConfig::data_dir().join("tmux-v3")
}
fn ensure_root() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(root())?;
    fs::set_permissions(root(), fs::Permissions::from_mode(0o700))?;
    Ok(())
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn session_path(id: &str) -> Result<PathBuf> {
    anyhow::ensure!(valid_id(id), "Invalid session ID");
    Ok(root().join(format!("{id}.json")))
}
fn saved(id: &str) -> Result<SavedSession> {
    Ok(serde_json::from_slice(&fs::read(session_path(id)?)?)?)
}
fn control_path(id: &str) -> PathBuf {
    // Short enough for Unix-domain sockets, even with long home paths.
    let uid = std::os::unix::fs::MetadataExt::uid(&fs::metadata(root()).expect("state directory"));
    PathBuf::from(format!("/tmp/essh-{uid}/{id}"))
}
fn ensure_control_dir(id: &str) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let dir = control_path(id).parent().unwrap().to_owned();
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let m = fs::symlink_metadata(&dir)?;
    anyhow::ensure!(
        m.is_dir() && m.uid() == fs::metadata(root())?.uid() && m.permissions().mode() & 0o077 == 0,
        "Unsafe SSH socket directory {}",
        dir.display()
    );
    Ok(())
}

pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn helper(args: &[&str]) -> Result<String> {
    let exe = std::env::current_exe()?;
    Ok(std::iter::once(quote(&exe.to_string_lossy()))
        .chain(std::iter::once("tmux".into()))
        .chain(args.iter().map(|s| quote(s)))
        .collect::<Vec<_>>()
        .join(" "))
}
fn tmux() -> Command {
    let mut cmd = Command::new("tmux");
    cmd.args(["-L", "essh-v3", "-f", "/dev/null"]);
    cmd
}
pub fn call(args: &[&str]) -> Result<String> {
    let out = tmux()
        .args(args)
        .output()
        .context("tmux is required (version 3.3 or newer)")?;
    anyhow::ensure!(
        out.status.success(),
        "tmux: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn start() -> Result<()> {
    use std::io::IsTerminal;
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "essh needs an interactive terminal"
    );
    ensure_root()?;
    Command::new("ssh")
        .arg("-V")
        .output()
        .context("OpenSSH is required")?;
    if call(&["has-session", "-t", "=essh"]).is_err() {
        call(&[
            "new-session",
            "-d",
            "-s",
            "essh",
            "-n",
            "hosts",
            &helper(&["list"])?,
        ])?;
    }
    configure()?;
    // Local reboot or server loss must not lose the remote session identities.
    let live = windows()?;
    let mut records = Vec::new();
    for entry in fs::read_dir(root())? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(record) = serde_json::from_slice::<SavedSession>(&fs::read(path)?) {
                records.push(record);
            }
        }
    }
    records.sort_by_key(|r| (r.index.unwrap_or(u32::MAX), r.created));
    for record in records {
        if valid_id(&record.id) && !live.iter().any(|w| w.id == record.id) {
            validate_host(&record.host)?;
            create_window(&record, false)?;
        }
    }
    call(&["select-window", "-t", "essh:0"])?;
    let status = tmux()
        .env_remove("TMUX")
        .args(["attach-session", "-t", "=essh"])
        .status()?;
    anyhow::ensure!(status.success(), "tmux client exited with {status}");
    let sessions = windows().unwrap_or_default();
    let hosts: std::collections::HashSet<_> = sessions.iter().map(|w| &w.host).collect();
    println!("[detached from essh]\n{} saved sessions on {} hosts. Remote jobs were not terminated. Run essh to return.",sessions.len(),hosts.len());
    Ok(())
}

fn configure() -> Result<()> {
    for (key, val) in [
        ("prefix", "C-a"),
        ("status-style", "bg=#0d1316,fg=#8b9aa6"),
        ("status-left", "#[fg=#4fd1c5,bold] essh #[default]"),
        ("status-left-length", "8"),
        ("status-right-length", "40"),
        ("status-interval", "5"),
        ("escape-time", "50"),
        ("base-index", "0"),
        ("renumber-windows", "off"),
        ("mouse", "off"),
        ("default-terminal", "tmux-256color"),
        ("history-limit", "10000"),
    ] {
        call(&["set-option", "-g", key, val])?;
    }
    for (key, val) in [
        ("automatic-rename", "off"), ("allow-rename", "off"),
        ("pane-border-style", "fg=#1c2429"), ("pane-active-border-style", "fg=#1c2429"),
        ("window-style", "bg=#080b0d,fg=#cbd5dd"),
        ("window-status-format", " #{?#{==:#I,0},hosts,#I #{@essh_host} #{@essh_dot}} "),
        ("window-status-current-format", "#[bg=#10171b,fg=#4fd1c5,bold] #{?#{==:#I,0},hosts,#I #{@essh_host} #{@essh_dot}} #[default]"),
    ] { call(&["set-window-option", "-g", key, val])?; }
    call(&[
        "set-option",
        "-g",
        "status-right",
        &format!("#({})", helper(&["status", "#{window_id}"])?),
    ])?;
    call(&["unbind-key", "C-b"])?;
    call(&["bind-key", "C-a", "send-prefix"])?;
    call(&["bind-key", "d", "select-window", "-t", "essh:0"])?;
    call(&["bind-key", "q", "detach-client"])?;
    for (key, action) in [("s", "switch"), ("?", "help")] {
        call(&[
            "bind-key",
            key,
            "display-popup",
            "-E",
            "-w",
            "90%",
            "-h",
            "80%",
            "-b",
            "single",
            "-S",
            "fg=#4fd1c5",
            &helper(&[action])?,
        ])?;
    }
    call(&[
        "bind-key",
        "n",
        "display-popup",
        "-E",
        "-w",
        "90%",
        "-h",
        "80%",
        "-b",
        "single",
        "-S",
        "fg=#4fd1c5",
        &helper(&["list", "--new"])?,
    ])?;
    call(&[
        "bind-key",
        "-n",
        "F2",
        "run-shell",
        "-b",
        &helper(&["toggle", "#{window_id}"])?,
    ])?;
    call(&[
        "bind-key",
        "m",
        "run-shell",
        "-b",
        &helper(&["toggle", "#{window_id}"])?,
    ])?;
    call(&[
        "bind-key",
        "x",
        "display-popup",
        "-E",
        "-w",
        "90%",
        "-h",
        "60%",
        &helper(&["kill", "current"])?,
    ])?;
    navigation::configure()?;
    let sync = format!(
        "run-shell -b {}",
        quote(&helper(&["sync-side", "#{window_id}"])?)
    );
    for hook in ["after-select-window", "client-resized"] {
        call(&["set-hook", "-g", hook, &sync])?;
    }
    call(&[
        "bind-key",
        "h",
        "display-popup",
        "-E",
        "-w",
        "100%",
        "-h",
        "100%",
        "-B",
        &helper(&["view", "health"])?,
    ])?;
    Ok(())
}

pub fn windows() -> Result<Vec<Window>> {
    let text = call(&["list-windows", "-t", "=essh", "-F", "#{window_id}\t#{window_index}\t#{@essh_id}\t#{@essh_host}\t#{window_active}\t#{@essh_state}"])?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let p: Vec<_> = line.split('\t').collect();
            if p.len() < 6 || !valid_id(p[2]) {
                return None;
            }
            Some(Window {
                target: p[0].into(),
                index: p[1].into(),
                id: p[2].into(),
                host: p[3].into(),
                active: p[4] == "1",
                state: p[5].into(),
            })
        })
        .collect())
}
fn option(window: &str, key: &str, value: &str) -> Result<()> {
    call(&["set-window-option", "-t", window, key, value])?;
    Ok(())
}
pub fn select(window: &str) -> Result<()> {
    call(&["select-window", "-t", window])?;
    Ok(())
}

pub fn open(host: &Host, new: bool) -> Result<()> {
    ensure_root()?;
    validate_host(host)?;
    let current = windows()?;
    if !new {
        if let Some(w) = current
            .iter()
            .filter(|w| w.host == host.name)
            .max_by_key(|w| (w.active, w.state == "connected"))
        {
            select(&w.target)?;
            if w.state.starts_with("disconnected") || w.state.starts_with("saved") {
                let pane = call(&["show-window-options", "-v", "-t", &w.target, "@essh_main"])?;
                call(&["send-keys", "-t", &pane, "Enter"])?;
            }
            return Ok(());
        }
    }
    let record = SavedSession {
        id: uuid::Uuid::new_v4().simple().to_string(),
        host: host.clone(),
        created: chrono::Utc::now().timestamp(),
        index: None,
    };
    fs::write(session_path(&record.id)?, serde_json::to_vec(&record)?)?;
    create_window(&record, true)?;
    Ok(())
}

fn create_window(record: &SavedSession, connect: bool) -> Result<()> {
    let host = &record.host;
    let requested = record
        .index
        .map(|n| format!("essh:{n}"))
        .unwrap_or_else(|| "essh:".into());
    // Create metadata before starting the helper: it uses this window's identity.
    let target = call(&[
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
        "-t",
        &requested,
        "-n",
        &host.name,
        "sleep 86400",
    ])?;
    let mut record = record.clone();
    record.index = call(&["display-message", "-p", "-t", &target, "#{window_index}"])?
        .parse()
        .ok();
    fs::write(session_path(&record.id)?, serde_json::to_vec(&record)?)?;
    let pane = call(&["display-message", "-p", "-t", &target, "#{pane_id}"])?;
    option(&target, "@essh_main", &pane)?;
    option(&target, "@essh_id", &record.id)?;
    option(&target, "@essh_host", &safe_label(&host.name))?;
    option(&target, "@essh_state", "connecting")?;
    option(&target, "@essh_dot", "○")?;
    call(&[
        "respawn-pane",
        "-k",
        "-t",
        &target,
        &helper(&if connect {
            vec!["attach", &record.id]
        } else {
            vec!["attach", &record.id, "--wait"]
        })?,
    ])?;
    select(&target)
}
fn safe_label(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && *c != '#')
        .take(80)
        .collect()
}
fn validate_host(host: &Host) -> Result<()> {
    for s in [&host.name, &host.hostname] {
        anyhow::ensure!(
            !s.is_empty()
                && !s.starts_with('-')
                && !s.chars().any(|c| c.is_control() || c.is_whitespace()),
            "Invalid host name"
        );
    }
    anyhow::ensure!(host.port != 0, "Port must be nonzero");
    Ok(())
}
fn ssh_command(s: &SavedSession) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "RemoteCommand=none",
    ]);
    cmd.arg("-S").arg(control_path(&s.id));
    if !s.host.via_ssh_config {
        cmd.arg("-p").arg(s.host.port.to_string());
        if let Some(user) = &s.host.user {
            cmd.arg("-l").arg(user);
        }
        if let Some(key) = &s.host.key {
            cmd.arg("-i").arg(shellexpand::tilde(key).as_ref());
        }
        if let Some(jump) = &s.host.jump {
            cmd.arg("-J").arg(jump);
        }
    }
    cmd
}
fn destination(s: &SavedSession) -> &str {
    if s.host.via_ssh_config {
        &s.host.name
    } else {
        &s.host.hostname
    }
}
fn remote_attach(id: &str) -> String {
    // Session names are generated hex, never host-supplied shell text. -L isolates
    // the app from the user's usual remote tmux server and configuration.
    format!("command -v tmux >/dev/null || {{ echo 'essh requires tmux on this host'; exit 127; }}; tmux -L essh -f /dev/null new-session -Ad -s essh-{id} && tmux -L essh set-option -t '=essh-{id}:' status off && exec tmux -L essh attach-session -t '=essh-{id}'")
}
fn attach(id: &str, wait: bool) -> Result<()> {
    let s = saved(id)?;
    validate_host(&s.host)?;
    ensure_control_dir(id)?;
    let window = windows()?
        .into_iter()
        .find(|w| w.id == id)
        .context("Session window missing")?
        .target;
    if wait {
        option(&window, "@essh_state", "saved · remote state unknown")?;
        println!(
            "Saved session on {}. Press Enter to reconnect.",
            s.host.name
        );
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
    }
    loop {
        option(&window, "@essh_state", "connecting")?;
        println!("Connecting to {}…", s.host.name);
        let child = ssh_command(&s)
            .args([
                "-tt",
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPersist=30",
                "--",
                destination(&s),
                &remote_attach(id),
            ])
            .spawn()?;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sampler = metrics::spawn(s.clone(), window.clone(), stop.clone());
        let result = {
            let mut child = child;
            child.wait()
        };
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = sampler.join();

        option(
            &window,
            "@essh_state",
            "disconnected · remote state unknown",
        )?;
        option(&window, "@essh_dot", "#[fg=#ef5f52]●#[default]")?;
        println!("\r\n{} disconnected ({:?}). Remote session state is unverified.\r\nr or Enter reconnects; q returns to hosts. Reconnecting recreates the shell if it has ended.", s.host.name, result?);
        loop {
            if ui::reconnect_key()? {
                break;
            }
            navigation::main_menu()?;
        }
    }
}

fn side_mode() -> u8 {
    call(&["show-options", "-gv", "@essh_side_mode"])
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
fn toggle(window: &str) -> Result<()> {
    let mode = (side_mode() + 1) % 3;
    call(&["set-option", "-g", "@essh_side_mode", &mode.to_string()])?;
    sync_side(window)
}
fn sync_side(window: &str) -> Result<()> {
    let Some(w) = windows()?.into_iter().find(|w| w.target == window) else {
        return Ok(());
    };
    let mode = side_mode();
    let panes = call(&[
        "list-panes",
        "-t",
        window,
        "-F",
        "#{pane_id}\t#{@essh_strip}",
    ])?;
    let pane = panes.lines().find_map(|l| l.strip_suffix("\t1"));
    let width: u16 = call(&["display-message", "-p", "-t", window, "#{window_width}"])?.parse()?;
    let wanted = if width < 80 { 0 } else { mode };
    let applied = call(&[
        "show-window-options",
        "-v",
        "-t",
        window,
        "@essh_side_applied",
    ])
    .unwrap_or_default();
    if let Some(pane) = pane {
        if wanted == 0 {
            call(&["kill-pane", "-t", pane])?;
        } else {
            let size = if wanted == 1 { 38 } else { width / 2 };
            call(&["resize-pane", "-t", pane, "-x", &size.to_string()])?;
            if applied != wanted.to_string() {
                let cmd = if wanted == 2 {
                    helper(&["strip", &w.id, "--half"])?
                } else {
                    helper(&["strip", &w.id])?
                };
                call(&["respawn-pane", "-k", "-t", pane, &cmd])?;
            }
        }
    } else if wanted > 0 {
        let size = if wanted == 1 { 38 } else { width / 2 };
        let cmd = if wanted == 2 {
            helper(&["strip", &w.id, "--half"])?
        } else {
            helper(&["strip", &w.id])?
        };
        let target = call(&["show-window-options", "-v", "-t", window, "@essh_main"])?;
        let pane = call(&[
            "split-window",
            "-h",
            "-d",
            "-l",
            &size.to_string(),
            "-P",
            "-F",
            "#{pane_id}",
            "-t",
            &target,
            &cmd,
        ])?;
        call(&["set-option", "-p", "-t", &pane, "@essh_strip", "1"])?;
    }
    option(window, "@essh_side_applied", &wanted.to_string())?;
    if mode > 0 && wanted == 0 {
        call(&[
            "display-message",
            "Side health needs 80 columns; F2 opens full health",
        ])?;
    }
    Ok(())
}

fn print_status(window: &str) {
    let time = chrono::Local::now().format("%H:%M").to_string();
    if let Ok(ws) = windows() {
        if let Some(w) = ws
            .iter()
            .find(|w| w.target == window && w.state == "connected")
        {
            if let Some(m) = metrics::read(&w.id).filter(|m| m.fresh()) {
                print!(
                    "↓ {} ↑ {} rtt {} {time}",
                    metrics::rate(m.rx),
                    metrics::rate(m.tx),
                    m.rtt
                        .map(|r| format!("{r:.0}ms"))
                        .unwrap_or_else(|| "—".into())
                );
                return;
            }
            print!("{time}");
            return;
        }
    }
    print!("{time}");
}

fn kill(window: &str) -> Result<()> {
    // display-popup does not expand formats in its shell command. Resolve the
    // active window here, before confirmation, and keep that exact target.
    let Some(w) = windows()?
        .into_iter()
        .find(|w| w.target == window || (window == "current" && w.active))
    else {
        return Ok(());
    };
    let window = w.target.as_str();
    if !ui::confirm_kill(&w)? {
        return Ok(());
    }
    let mut answer = String::new();
    let s = saved(&w.id)?;
    let command = format!("tmux -L essh kill-session -t '=essh-{}'", s.id);
    let status = ssh_command(&s)
        .args(["-T", "-o", "BatchMode=yes", "--", destination(&s), &command])
        .status()?;
    if !status.success() {
        println!("Could not confirm remote termination. Session retained. Press Enter.");
        answer.clear();
        io::stdin().read_line(&mut answer)?;
        bail!("Remote kill failed; session retained");
    }
    // Remove recovery state before closing the window hosting this popup.
    // Closing it may terminate this helper immediately.
    fs::remove_file(session_path(&s.id)?)?;
    let _ = fs::remove_file(root().join(format!("{}.metrics", s.id)));
    if w.active {
        let mut remaining: Vec<_> = windows()?.into_iter().filter(|v| v.id != w.id).collect();
        let index = w.index.parse::<u32>().unwrap_or(0);
        remaining.sort_by_key(|v| v.index.parse::<u32>().unwrap_or(0).abs_diff(index));
        select(
            remaining
                .first()
                .map(|v| v.target.as_str())
                .unwrap_or("essh:0"),
        )?;
    }
    call(&["kill-window", "-t", window])?;
    let _ = call(&["display-message", "Session killed"]);
    Ok(())
}

/// Bounded subprocess capture: a stalled remote command cannot freeze navigation
/// or leave a metrics worker behind after the interactive connection exits.
fn capture(mut cmd: Command, timeout: Duration) -> Result<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let reader = std::thread::spawn(move || {
        use io::Read;
        let mut bytes = Vec::new();
        stdout
            .take(2_000_000)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let bytes = reader
                .join()
                .map_err(|_| anyhow::anyhow!("reader failed"))??;
            anyhow::ensure!(status.success(), "probe failed");
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("probe timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
