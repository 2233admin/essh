use super::*;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Row, Table, TableState, Wrap},
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
#[derive(Clone, Copy)]
enum GraphScale {
    Traffic,
    Percent,
    Latency,
}
// Thin horizontal strokes leave space above and below the meter, unlike
// full-cell gauges. The track uses the reference's exact #14191d color.
fn thin_meter(value: Option<f64>, width: u16) -> Line<'static> {
    let Some(value) = value.filter(|v| v.is_finite()) else {
        return Line::styled("— usage unavailable", Style::default().fg(DIM));
    };
    let used = (value.clamp(0.0, 100.0) / 100.0 * f64::from(width)).round() as usize;
    Line::from(vec![
        Span::styled("─".repeat(used), Style::default().fg(GREEN).bg(BG)),
        Span::styled(
            "─".repeat(usize::from(width).saturating_sub(used)),
            Style::default().fg(Color::Rgb(20, 25, 29)).bg(BG),
        ),
    ])
}
fn graph_history(values: &[u64], width: usize, scale: GraphScale) -> String {
    let ceiling = match scale {
        GraphScale::Traffic => values.iter().copied().max().unwrap_or(1).max(1) as f64 * 1.6,
        GraphScale::Percent => 100.0,
        GraphScale::Latency => 50.0,
    };
    let glyphs = ['⣀', '⣄', '⣤', '⣶', '⣿'];
    let mut out = "⣀".repeat(width.saturating_sub(values.len()));
    for v in values.iter().skip(values.len().saturating_sub(width)) {
        out.push(glyphs[((*v as f64 / ceiling * 4.0).floor() as usize).min(4)]);
    }
    out
}
fn spark(values: &[u64], width: usize) -> String {
    graph_history(values, width, GraphScale::Traffic)
}
pub fn cached_history(name: &str) -> Vec<u64> {
    fs::read(probe_path(name).with_extension("history"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
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
pub(super) fn probe_path(name: &str) -> PathBuf {
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

fn host_form(config: &mut AppConfig, original: Option<&Host>) -> Result<Option<String>> {
    let mut screen = Screen::new()?;
    let mut field = 0;
    let mut values = original
        .map(|h| {
            vec![
                h.name.clone(),
                h.user.clone().unwrap_or_default(),
                h.hostname.clone(),
                h.port.to_string(),
            ]
        })
        .unwrap_or_else(|| vec![String::new(), String::new(), String::new(), "22".into()]);
    let mut error = String::new();
    let labels = ["Name", "User (empty = root)", "Address", "Port"];
    let mut fields = Vec::new();
    loop {
        screen.term.draw(|f| {
            background(f);
            let area = f.area();
            let width = area.width.min(62);
            let height = area.height.min(19);
            let popup = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            let title = if original.is_some() {
                " Edit host "
            } else {
                " Add host "
            };
            let block = Block::bordered()
                .title(title)
                .border_style(Style::default().fg(CYAN));
            let inner = block.inner(popup);
            f.render_widget(block, popup);
            let rows = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(inner);
            fields.clear();
            for i in 0..4 {
                fields.push(rows[i]);
                let block = Block::bordered()
                    .title(labels[i])
                    .border_style(Style::default().fg(if i == field { CYAN } else { DIM }));
                let inside = block.inner(rows[i]);
                let limit = usize::from(inside.width.saturating_sub(1));
                let chars: Vec<_> = values[i].chars().collect();
                let visible: String = chars
                    .iter()
                    .skip(chars.len().saturating_sub(limit))
                    .collect();
                f.render_widget(
                    Paragraph::new(visible.clone())
                        .block(block)
                        .style(Style::default().fg(TEXT)),
                    rows[i],
                );
                if i == field && inside.width > 0 && inside.height > 0 {
                    f.set_cursor_position(Position::new(
                        inside.x + visible.chars().count().min(limit) as u16,
                        inside.y,
                    ));
                }
            }
            line(
                f,
                rows[4],
                if !error.is_empty() {
                    error.clone()
                } else if original.is_some() {
                    "Changes apply to new connections".into()
                } else {
                    "Host reachability is checked after saving".into()
                },
                DIM,
            );
            line(
                f,
                rows[5],
                "Tab/click field · Enter save · Esc/F4 cancel · F1 help",
                DIM,
            );
        })?;
        if !event::poll(Duration::from_millis(150))? {
            continue;
        }
        let code = match event::read()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => k.code,
            Event::Mouse(m) if m.kind == event::MouseEventKind::Down(event::MouseButton::Left) => {
                if let Some(i) = fields
                    .iter()
                    .position(|r| r.contains(Position::new(m.column, m.row)))
                {
                    field = i;
                }
                continue;
            }
            _ => continue,
        };
        match code {
            KeyCode::Esc | KeyCode::F(4) => return Ok(None),
            KeyCode::F(1) => {
                error = "Tab/Shift+Tab fields; type to edit; Enter saves; Esc cancels".into()
            }
            KeyCode::Tab | KeyCode::Down => field = (field + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => field = (field + 3) % 4,
            KeyCode::Backspace => {
                values[field].pop();
            }
            KeyCode::Char(c) => values[field].push(c),
            KeyCode::Enter => {
                if values[0].trim().is_empty() || values[2].trim().is_empty() {
                    error = "Name and address are required".into();
                    continue;
                }
                if [&values[0], &values[2]]
                    .iter()
                    .any(|v| v.starts_with('-') || v.chars().any(char::is_whitespace))
                {
                    error = "Name/address cannot start with '-' or contain spaces".into();
                    continue;
                }
                if config
                    .hosts
                    .iter()
                    .any(|h| h.name == values[0] && original.is_none_or(|o| o.name != h.name))
                {
                    error = "A host with this name already exists".into();
                    continue;
                }
                let port = match values[3].parse::<u16>() {
                    Ok(p) if p > 0 => p,
                    _ => {
                        error = "Port must be 1–65535".into();
                        continue;
                    }
                };
                let position =
                    original.and_then(|h| config.hosts.iter().position(|v| v.name == h.name));
                let mut entry =
                    position
                        .map(|i| config.hosts[i].clone())
                        .unwrap_or(crate::config::HostEntry {
                            name: values[0].clone(),
                            hostname: values[2].clone(),
                            port,
                            user: None,
                            key: None,
                            tags: HashMap::new(),
                            jump_host: None,
                            port_forwards: Vec::new(),
                        });
                entry.name = values[0].clone();
                entry.hostname = values[2].clone();
                entry.port = port;
                entry.user = Some(if values[1].is_empty() {
                    "root".into()
                } else {
                    values[1].clone()
                });
                if let Some(i) = position {
                    config.hosts[i] = entry;
                } else {
                    config.hosts.push(entry);
                }
                config.save()?;
                return Ok(Some(values[0].clone()));
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
        let sample = metrics::read(id);
        let m = sample.as_ref().filter(|m| m.fresh());
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
                    (r[5], "disk", m.disk),
                ] {
                    match value {
                        Some(v) => {
                            let mut spans = vec![Span::styled(
                                format!("{name:<5} "),
                                Style::default().fg(DIM),
                            )];
                            spans.extend(thin_meter(Some(v), area.width.saturating_sub(12)).spans);
                            spans.push(Span::styled(
                                format!(" {v:>4.0}%"),
                                Style::default().fg(TEXT),
                            ));
                            line(f, area, Line::from(spans), TEXT);
                        }
                        None => line(f, area, format!("{name} —"), DIM),
                    }
                }
                line(
                    f,
                    r[6],
                    format!(
                        "rtt {} {}",
                        graph_history(&m.rtt_history, 14, GraphScale::Latency),
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
                line(
                    f,
                    r[1],
                    if sample.is_some() {
                        "Last reading · not live"
                    } else {
                        "Metrics unavailable"
                    },
                    DIM,
                );
            }
            line(f, r[8], "F2 half · ^A h full", DIM);
        })?;
        let _ = key()?;
    }
}

pub(super) fn reconnect_key() -> Result<bool> {
    enable_raw_mode()?;
    let _guard = scopeguard::guard((), |_| {
        let _ = disable_raw_mode();
    });
    loop {
        match key()? {
            Some(KeyCode::Enter | KeyCode::Char('r')) => return Ok(true),
            Some(KeyCode::Char('q')) => return Ok(false),
            _ => (),
        }
    }
}

pub(super) fn confirm_kill(w: &Window) -> Result<bool> {
    let mut screen = Screen::new()?;
    let command = metrics::read(&w.id)
        .filter(|m| m.fresh())
        .and_then(|m| m.running)
        .unwrap_or_else(|| "Remote programs".into());
    loop {
        screen.term.draw(|f| {
            background(f);let a=f.area();let width=a.width.min(64);let height=a.height.min(9);
            let r=Rect::new(a.x+(a.width-width)/2,a.y+(a.height-height)/2,width,height);
            let block=Block::bordered().title(format!(" Kill session {} on {}? ",w.index,w.host)).border_style(Style::default().fg(RED));
            let text=format!("{command} will be terminated.\nReturn to Hosts (F4) to keep it running.\n\ny Yes, kill · n / Esc Cancel");
            f.render_widget(Paragraph::new(text).block(block).wrap(Wrap{trim:false}).style(Style::default().fg(TEXT)),r);
        })?;
        match key()? {
            Some(KeyCode::Char('y')) => return Ok(true),
            Some(KeyCode::Esc | KeyCode::Char('n') | KeyCode::F(4)) => return Ok(false),
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graphs_use_fixed_scales_and_left_padding() {
        assert_eq!(graph_history(&[4, 8, 4], 5, GraphScale::Latency), "⣀⣀⣀⣀⣀");
        assert_eq!(graph_history(&[0, 50, 100], 3, GraphScale::Percent), "⣀⣤⣿");
        assert_eq!(graph_history(&[10, 10], 4, GraphScale::Traffic), "⣀⣀⣤⣤");
        assert_eq!(graph_history(&[500], 1, GraphScale::Latency), "⣿");
    }
}
