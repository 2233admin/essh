use super::*;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph, Row, Table, TableState, Wrap},
};
use std::time::Instant;
use std::{
    collections::HashMap,
    net::{TcpStream, ToSocketAddrs},
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
        io::stdout().execute(EnableMouseCapture)?;
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
        let _ = io::stdout().execute(DisableMouseCapture);
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

pub(super) mod workspace;
pub fn hosts(config: AppConfig, new: bool) -> Result<()> {
    workspace::run(config, navigation::View::Hosts, new, new)
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
    workspace::run(AppConfig::load()?, navigation::View::Sessions, true, false)
}
pub fn help() -> Result<()> {
    workspace::run(AppConfig::load()?, navigation::View::Help, true, false)
}

pub fn strip(id: &str) -> Result<()> {
    let s = saved(id)?;
    let mut screen = Screen::new()?;
    loop {
        let m = metrics::read(id).filter(|m| m.fresh());
        screen.term.draw(|f| {
            background(f);
            if f.area().width < 26 {
                line(f, f.area(), "Resize · ^A m hide", DIM);
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
            line(f, r[8], "^A m hide · F2 Health", DIM);
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
