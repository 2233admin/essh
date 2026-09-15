use super::*;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph, Row, Table, TableState, Wrap},
};
use std::{
    collections::HashMap,
    net::{TcpStream, ToSocketAddrs},
    sync::mpsc,
    time::Instant,
};

const BG: Color = Color::Rgb(8, 11, 13);
const TEXT: Color = Color::Rgb(203, 213, 221);
const DIM: Color = Color::Rgb(91, 107, 122);
const CYAN: Color = Color::Rgb(79, 209, 197);
const GREEN: Color = Color::Rgb(53, 208, 127);
const RED: Color = Color::Rgb(239, 95, 82);
const SELECTED: Color = Color::Rgb(16, 23, 27);

type Term = Terminal<CrosstermBackend<io::Stdout>>;
struct Screen {
    term: Term,
}
impl Screen {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(e) = io::stdout().execute(EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(term) => Ok(Self { term }),
            Err(e) => {
                let _ = disable_raw_mode();
                let _ = io::stdout().execute(LeaveAlternateScreen);
                Err(e.into())
            }
        }
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = self.term.show_cursor();
    }
}
fn key() -> Result<Option<KeyCode>> {
    if event::poll(Duration::from_millis(150))? {
        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Release {
                return Ok(Some(k.code));
            }
        }
    }
    Ok(None)
}
fn background(f: &mut Frame) {
    f.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        f.area(),
    );
}
fn line(f: &mut Frame, area: Rect, text: impl Into<Line<'static>>, color: Color) {
    f.render_widget(
        Paragraph::new(text.into()).style(Style::default().fg(color)),
        area,
    );
}
fn main_layout(area: Rect) -> Vec<Rect> {
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .margin(1)
    .split(area)
    .to_vec()
}
fn spark(values: &[u64], width: usize) -> String {
    let max = values.iter().copied().max().unwrap_or(1).max(1);
    let glyphs = ['⣀', '⣄', '⣠', '⣤', '⣶', '⣿'];
    values
        .iter()
        .skip(values.len().saturating_sub(width))
        .map(|v| glyphs[((*v as f64 / max as f64) * 5.0) as usize])
        .collect()
}

pub fn tcp_latency(host: &Host) -> Option<f64> {
    if host.proxy {
        return None;
    }
    let start = Instant::now();
    let addrs = (host.hostname.as_str(), host.port).to_socket_addrs().ok()?;
    for addr in addrs.take(2) {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(700)).is_ok() {
            return Some(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    None
}
fn probe_path(name: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    root().join(format!(
        "probe-{}",
        hex::encode(Sha256::digest(name.as_bytes()))
    ))
}
pub fn cached_latency(name: &str) -> Option<f64> {
    let (at, latency): (i64, Option<f64>) =
        serde_json::from_slice(&fs::read(probe_path(name)).ok()?).ok()?;
    if chrono::Utc::now().timestamp() - at > 15 {
        None
    } else {
        latency
    }
}
#[derive(Default)]
struct Probe {
    latency: Option<f64>,
    history: Vec<u64>,
    checked: bool,
}

pub fn hosts(mut config: AppConfig, new: bool) -> Result<()> {
    let mut screen = Screen::new()?;
    let mut list = super::hosts(&config);
    let mut probes: HashMap<String, Probe> = HashMap::new();
    let (tx, rx) = mpsc::channel::<Option<(String, Option<f64>)>>();
    let mut probe_inflight = false;
    let mut next = Instant::now();
    let mut selected = 0;
    let mut filter = String::new();
    let mut filtering = false;
    let mut message = String::new();
    let mut table_state = TableState::default();
    let mut session_windows = windows().unwrap_or_default();
    let mut refreshed = Instant::now();
    loop {
        while let Ok(result) = rx.try_recv() {
            match result {
                Some((name, latency)) => {
                    if let Ok(bytes) =
                        serde_json::to_vec(&(chrono::Utc::now().timestamp(), latency))
                    {
                        let _ = fs::write(probe_path(&name), bytes);
                    }
                    for w in session_windows
                        .iter()
                        .filter(|w| w.host == name && w.state != "connected")
                    {
                        let dot = if latency.is_some() {
                            "#[fg=#35d07f]●#[default]"
                        } else {
                            "#[fg=#ef5f52]●#[default]"
                        };
                        let _ = option(&w.target, "@essh_dot", dot);
                    }
                    let p: &mut Probe = probes.entry(name).or_default();
                    p.latency = latency;
                    p.checked = true;
                    if let Some(ms) = latency {
                        p.history.push(ms as u64);
                        if p.history.len() > 10 {
                            p.history.remove(0);
                        }
                    }
                }
                None => {
                    probe_inflight = false;
                    next = Instant::now() + Duration::from_secs(5);
                }
            }
        }
        if !probe_inflight && Instant::now() >= next {
            probe_inflight = true;
            let hosts = list.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Bounded parallelism, never one thread per configured host.
                for chunk in hosts.chunks(8) {
                    std::thread::scope(|scope| {
                        for h in chunk {
                            let tx = tx.clone();
                            scope.spawn(move || {
                                let _ = tx.send(Some((h.name.clone(), tcp_latency(h))));
                            });
                        }
                    });
                }
                let _ = tx.send(None);
            });
        }
        if refreshed.elapsed() > Duration::from_secs(1) {
            session_windows = windows().unwrap_or_default();
            refreshed = Instant::now();
        }
        let query = filter.to_lowercase();
        let shown: Vec<_> = list
            .iter()
            .filter(|h| {
                format!("{} {}", h.name, h.tags)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        selected = selected.min(shown.len().saturating_sub(1));
        table_state.select(if shown.is_empty() {
            None
        } else {
            Some(selected)
        });
        screen.term.draw(|f| {
            background(f);
            let area = main_layout(f.area());
            line(
                f,
                area[0],
                format!(
                    "essh │ {} hosts · {} sessions{}",
                    list.len(),
                    session_windows.len(),
                    if new { " · new session" } else { "" }
                ),
                CYAN,
            );
            let wide = area[1].width >= 105;
            let medium = area[1].width >= 65;
            let rows = shown.iter().map(|h| {
                let p = probes.get(&h.name);
                let sessions: Vec<_> = session_windows
                    .iter()
                    .filter(|w| w.host == h.name)
                    .collect();
                let connected = sessions.iter().any(|w| w.state == "connected");
                let reachable = connected || p.is_some_and(|p| p.latency.is_some());
                let color = if reachable {
                    GREEN
                } else if !sessions.is_empty() {
                    RED
                } else {
                    DIM
                };
                let dot = if reachable || !sessions.is_empty() {
                    "●"
                } else {
                    "○"
                };
                let latency = if h.proxy {
                    if connected {
                        "via SSH".into()
                    } else {
                        "via proxy · unknown".into()
                    }
                } else if let Some(ms) = p.and_then(|p| p.latency) {
                    format!("{} {ms:.0}ms", spark(&p.unwrap().history, 10))
                } else if p.is_some_and(|p| p.checked) {
                    if sessions.is_empty() {
                        "offline".into()
                    } else {
                        "unreachable".into()
                    }
                } else {
                    "checking…".into()
                };
                let last: String = if sessions.is_empty() {
                    "—".into()
                } else if connected {
                    sessions
                        .iter()
                        .filter_map(|w| metrics::read(&w.id))
                        .find_map(|m| m.running)
                        .unwrap_or_else(|| "attached".into())
                } else {
                    "session saved · state unknown".into()
                };
                let mut cells = vec![
                    ratatui::widgets::Cell::from(dot).style(Style::default().fg(color)),
                    ratatui::widgets::Cell::from(h.name.clone()),
                ];
                if wide {
                    cells.push(
                        ratatui::widgets::Cell::from(format!(
                            "{}@{}",
                            h.user.as_deref().unwrap_or("—"),
                            h.hostname
                        ))
                        .style(Style::default().fg(DIM)),
                    );
                }
                cells.push(
                    ratatui::widgets::Cell::from(if sessions.is_empty() {
                        "·".into()
                    } else {
                        sessions.len().to_string()
                    })
                    .style(Style::default().fg(CYAN)),
                );
                cells.push(ratatui::widgets::Cell::from(latency).style(Style::default().fg(color)));
                if medium {
                    cells.push(ratatui::widgets::Cell::from(last).style(Style::default().fg(DIM)));
                }
                Row::new(cells)
            });
            let mut widths = vec![
                Constraint::Length(1),
                Constraint::Length(if medium { 16 } else { 14 }),
            ];
            let mut headers = vec!["", "HOST"];
            if wide {
                widths.push(Constraint::Length(24));
                headers.push("USER@ADDRESS");
            }
            widths.push(Constraint::Length(5));
            headers.push("TMUX");
            widths.push(if medium {
                Constraint::Length(18)
            } else {
                Constraint::Min(1)
            });
            headers.push("LATENCY");
            if medium {
                widths.push(Constraint::Min(1));
                headers.push("LAST");
            }
            let table = Table::new(rows, widths)
                .header(
                    Row::new(headers)
                        .style(Style::default().fg(DIM))
                        .bottom_margin(1),
                )
                .column_spacing(2)
                .row_highlight_style(Style::default().bg(SELECTED).fg(Color::White).bold())
                .highlight_symbol(Span::styled("▌", Style::default().fg(CYAN)));
            f.render_stateful_widget(table, area[1], &mut table_state);
            let footer = if filtering {
                format!("/ {filter}   Esc clears · Enter finishes")
            } else if !message.is_empty() {
                message.clone()
            } else if list.is_empty() {
                "No hosts. a add host · ? help · q quit".into()
            } else {
                "⏎ connect · a add · e edit · / filter · ? help · q quit".into()
            };
            f.render_widget(
                Paragraph::new(footer)
                    .style(Style::default().fg(DIM))
                    .wrap(Wrap { trim: false }),
                area[2],
            );
        })?;
        match key()? {
            Some(KeyCode::Esc) if filtering => {
                filter.clear();
                filtering = false;
            }
            Some(KeyCode::Enter) if filtering => {
                filtering = false;
            }
            Some(KeyCode::Backspace) if filtering => {
                filter.pop();
                selected = 0;
            }
            Some(KeyCode::Char(c)) if filtering => {
                filter.push(c);
                selected = 0;
            }
            Some(KeyCode::Char('/')) => filtering = true,
            Some(KeyCode::Down | KeyCode::Char('j')) => {
                selected = (selected + 1).min(shown.len().saturating_sub(1))
            }
            Some(KeyCode::Up | KeyCode::Char('k')) => selected = selected.saturating_sub(1),
            Some(KeyCode::Char('q') | KeyCode::Esc) => {
                if new {
                    return Ok(());
                }
                call(&["detach-client"])?;
            }
            Some(KeyCode::Char('?')) => {
                drop(screen);
                help()?;
                screen = Screen::new()?;
            }
            Some(KeyCode::Enter) => {
                if let Some(h) = shown.get(selected) {
                    match open(h, new) {
                        Ok(()) => {
                            if new {
                                return Ok(());
                            }
                            message.clear();
                        }
                        Err(e) => message = e.to_string(),
                    }
                }
            }
            Some(KeyCode::Char(c @ ('a' | 'e'))) => {
                let original = if c == 'e' {
                    shown.get(selected).map(|h| (*h).clone())
                } else {
                    None
                };
                drop(screen);
                // Both paths can add a new alias or update the selected alias;
                // the form asks for the name explicitly and preserves other fields.
                let result = host_form(&mut config, original.as_ref());
                screen = Screen::new()?;
                message = result.err().map(|e| e.to_string()).unwrap_or_default();
                list = super::hosts(&config);
            }
            _ => (),
        }
    }
}

fn host_form(config: &mut AppConfig, original: Option<&Host>) -> Result<()> {
    let mut screen = Screen::new()?;
    let mut field = 0;
    let mut values = if let Some(h) = original {
        vec![
            h.name.clone(),
            if h.via_ssh_config {
                h.name.clone()
            } else {
                h.hostname.clone()
            },
            h.user.clone().unwrap_or_default(),
            h.port.to_string(),
        ]
    } else {
        vec![String::new(), String::new(), String::new(), "22".into()]
    };
    let labels = ["Alias", "Hostname", "User (optional)", "Port"];
    let mut error = String::new();
    loop {
        screen.term.draw(|f| {
            background(f);
            let r = main_layout(f.area());
            line(f, r[0], "host · saved in essh configuration", CYAN);
            let rows = Layout::vertical([Constraint::Length(2); 4]).split(r[1]);
            for i in 0..4 {
                line(
                    f,
                    rows[i],
                    format!(
                        "{} {}: {}",
                        if i == field { "▌" } else { " " },
                        labels[i],
                        values[i]
                    ),
                    if i == field { CYAN } else { TEXT },
                );
            }
            line(
                f,
                r[2],
                if error.is_empty() {
                    "Tab next · Enter save · Esc cancel".into()
                } else {
                    error.clone()
                },
                DIM,
            );
        })?;
        match key()? {
            Some(KeyCode::Esc) => return Ok(()),
            Some(KeyCode::Tab | KeyCode::Down) => field = (field + 1) % 4,
            Some(KeyCode::BackTab | KeyCode::Up) => field = (field + 3) % 4,
            Some(KeyCode::Backspace) => {
                values[field].pop();
            }
            Some(KeyCode::Char(c)) => values[field].push(c),
            Some(KeyCode::Enter) => {
                let port = match values[3].parse::<u16>() {
                    Ok(n) if n > 0 => n,
                    _ => {
                        error = "Port must be 1–65535".into();
                        continue;
                    }
                };
                if values[0].is_empty()
                    || values[1].is_empty()
                    || values[..2]
                        .iter()
                        .any(|v| v.starts_with('-') || v.chars().any(char::is_whitespace))
                {
                    error =
                        "Alias and hostname must be nonempty without spaces or leading '-'".into();
                    continue;
                }
                let mut entry = config
                    .hosts
                    .iter()
                    .find(|h| h.name == values[0])
                    .cloned()
                    .unwrap_or(crate::config::HostEntry {
                        name: values[0].clone(),
                        hostname: values[1].clone(),
                        port,
                        user: None,
                        key: None,
                        tags: HashMap::new(),
                        jump_host: None,
                        port_forwards: Vec::new(),
                    });
                entry.hostname = values[1].clone();
                entry.port = port;
                entry.user = if values[2].is_empty() {
                    None
                } else {
                    Some(values[2].clone())
                };
                config.hosts.retain(|h| h.name != entry.name);
                config.hosts.push(entry);
                config.save()?;
                return Ok(());
            }
            _ => (),
        }
    }
}

pub fn switcher() -> Result<()> {
    let mut screen = Screen::new()?;
    let mut selected = 0;
    let mut state = TableState::default();
    let mut message = String::new();
    loop {
        let ws = windows()?;
        selected = selected.min(ws.len().saturating_sub(1));
        state.select(if ws.is_empty() { None } else { Some(selected) });
        screen.term.draw(|f| {
            background(f);
            let area = main_layout(f.area());
            line(f, area[0], "switch", CYAN);
            let rows = ws.iter().map(|w| {
                Row::new([
                    w.index.clone(),
                    w.host.clone(),
                    w.state.clone(),
                    if w.active {
                        "here".into()
                    } else {
                        String::new()
                    },
                ])
            });
            f.render_stateful_widget(
                Table::new(
                    rows,
                    [
                        Constraint::Length(4),
                        Constraint::Length(16),
                        Constraint::Min(1),
                        Constraint::Length(5),
                    ],
                )
                .column_spacing(1)
                .highlight_symbol(Span::styled("▌", Style::default().fg(CYAN)))
                .row_highlight_style(Style::default().bg(SELECTED).fg(CYAN)),
                area[1],
                &mut state,
            );
            line(
                f,
                area[2],
                if message.is_empty() {
                    "1–9 jump · ⏎ switch · n new · x kill · Esc close".into()
                } else {
                    message.clone()
                },
                DIM,
            );
        })?;
        match key()? {
            Some(KeyCode::Esc) => return Ok(()),
            Some(KeyCode::Down | KeyCode::Char('j')) => {
                selected = (selected + 1).min(ws.len().saturating_sub(1))
            }
            Some(KeyCode::Up | KeyCode::Char('k')) => selected = selected.saturating_sub(1),
            Some(KeyCode::Char(c @ '1'..='9')) => {
                if let Some(w) = ws.iter().find(|w| w.index == c.to_string()) {
                    select(&w.target)?;
                    return Ok(());
                }
            }
            Some(KeyCode::Enter) => {
                if let Some(w) = ws.get(selected) {
                    select(&w.target)?;
                    return Ok(());
                }
            }
            Some(KeyCode::Char('n')) => {
                drop(screen);
                return hosts(AppConfig::load()?, true);
            }
            Some(KeyCode::Char('x')) => {
                if let Some(w) = ws.get(selected) {
                    drop(screen);
                    message = kill(&w.target)
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    screen = Screen::new()?;
                }
            }
            _ => (),
        }
    }
}

pub fn help() -> Result<()> {
    let mut screen = Screen::new()?;
    let mut scroll = 0u16;
    loop {
        screen.term.draw(|f| {
            background(f);let r=main_layout(f.area());line(f,r[0],"essh keys",CYAN);
            f.render_widget(Paragraph::new("HOST LIST\n↑↓ jk  move     Enter  connect or reattach\n/  filter      a  add host     e  edit host\nq  quit; sessions keep running\n\nIN A SESSION — prefix Ctrl+A\n1–9  jump      s  switcher     n  new session\nm  strip       d  hosts        x  kill (confirmed)\nq  detach      ?  help         Ctrl+A  send prefix\n\nRemote tmux keeps its Ctrl+B prefix. Local essh shortcuts\nreplace some defaults; other tmux bindings remain available.\n\n● reachable  ● unreachable with saved session  ○ offline\nA saved session does not prove remote programs are alive.\nMetrics: current host only; hosts screen shows the clock.\nStrip: all non-loopback interfaces; TCP peer rates when available.")
                .style(Style::default().fg(TEXT)).wrap(Wrap{trim:false}).scroll((scroll, 0)),r[1]);
            line(f,r[2],"↑↓ scroll · Esc close",DIM);
        })?;
        match key()? {
            Some(KeyCode::Esc | KeyCode::Char('q')) => return Ok(()),
            Some(KeyCode::Down | KeyCode::Char('j')) => scroll = scroll.saturating_add(1).min(30),
            Some(KeyCode::Up | KeyCode::Char('k')) => scroll = scroll.saturating_sub(1),
            _ => (),
        }
    }
}

pub fn strip(id: &str) -> Result<()> {
    let s = saved(id)?;
    let mut screen = Screen::new()?;
    loop {
        let m = metrics::read(id).filter(|m| m.fresh());
        screen.term.draw(|f| {
            background(f);
            if f.area().width < 26 {
                line(f, f.area(), "Resize or ^A m hide", DIM);
                return;
            }
            let inner = f.area().inner(Margin::new(1, 1));
            let r = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
            line(f, r[0], format!("{}  · all interfaces", s.host.name), CYAN);
            if let Some(m) = &m {
                f.render_widget(
                    Paragraph::new(format!(
                        "RX  {}/s\n{}",
                        metrics::rate(m.rx),
                        spark(&m.rx_history, 27)
                    ))
                    .style(Style::default().fg(GREEN)),
                    r[1],
                );
                f.render_widget(
                    Paragraph::new(format!(
                        "TX  {}/s\n{}",
                        metrics::rate(m.tx),
                        spark(&m.tx_history, 27)
                    ))
                    .style(Style::default().fg(CYAN)),
                    r[2],
                );
                for (area, name, value) in [
                    (r[3], "cpu", m.cpu),
                    (r[4], "mem", m.mem),
                    (r[5], "disk /", m.disk),
                ] {
                    match value {
                        Some(v) => f.render_widget(
                            Gauge::default()
                                .ratio((v / 100.0).clamp(0.0, 1.0))
                                .label(format!("{name} {v:.0}%"))
                                .gauge_style(Style::default().fg(GREEN).bg(SELECTED)),
                            area,
                        ),
                        None => line(f, area, format!("{name} —"), DIM),
                    }
                }
                line(
                    f,
                    r[6],
                    format!(
                        "rtt {}",
                        m.rtt
                            .map(|r| format!("{r:.0}ms"))
                            .unwrap_or_else(|| "—".into())
                    ),
                    DIM,
                );
                let rows = m.peers.iter().map(|(p, rx, tx)| {
                    Row::new([
                        p.clone(),
                        metrics::rate(Some(*rx)),
                        metrics::rate(Some(*tx)),
                    ])
                });
                f.render_widget(
                    Table::new(
                        rows,
                        [
                            Constraint::Min(1),
                            Constraint::Length(6),
                            Constraint::Length(6),
                        ],
                    )
                    .header(Row::new(["PEER", "RX/s", "TX/s"]).style(Style::default().fg(DIM)))
                    .column_spacing(1)
                    .block(
                        Block::default()
                            .borders(Borders::TOP)
                            .border_style(Style::default().fg(DIM)),
                    ),
                    r[7],
                );
                if m.peers.is_empty() && r[7].height > 2 {
                    line(
                        f,
                        Rect::new(r[7].x, r[7].y + 2, r[7].width, 1),
                        "No peer counters available",
                        DIM,
                    );
                }
            } else {
                line(f, r[1], "Metrics unavailable", DIM);
            }
            line(f, r[8], "^A m hide", DIM);
        })?;
        let _ = key()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_terminal_layouts_remain_in_bounds() {
        for (w, h) in [(40, 12), (80, 24), (120, 40)] {
            let area = Rect::new(0, 0, w, h);
            for r in main_layout(area) {
                assert!(r.right() <= w && r.bottom() <= h);
            }
        }
    }
}
