//! Character-grid rendering of the full health reference.
use super::*;

const BORDER: Color = Color::Rgb(28, 36, 41);
const MID: Color = Color::Rgb(139, 154, 166);

fn text(value: impl Into<String>, color: Color) -> Line<'static> {
    Line::styled(value.into(), Style::default().fg(color))
}
fn value(value: impl Into<String>, color: Color) -> Line<'static> {
    Line::styled(value.into(), Style::default().fg(color).bold())
}
fn size(kb: Option<u64>) -> String {
    kb.map(|n| format!("{:.1} GiB", n as f64 / 1048576.0))
        .unwrap_or_else(|| "—".into())
}
fn rss(kb: u64) -> String {
    if kb >= 1048576 {
        format!("{:.1} GiB", kb as f64 / 1048576.0)
    } else if kb >= 1024 {
        format!("{:.1} MiB", kb as f64 / 1024.0)
    } else {
        format!("{kb} KiB")
    }
}
fn percent(n: Option<f64>) -> String {
    n.filter(|v| v.is_finite())
        .map(|v| format!("{v:.1}%"))
        .unwrap_or_else(|| "—".into())
}
fn rate(n: Option<f64>) -> String {
    match n.filter(|v| v.is_finite()) {
        Some(v) if v >= 1_000_000.0 => format!("{:.1} MB/s", v / 1_000_000.0),
        Some(v) if v >= 1_000.0 => format!("{:.1} kB/s", v / 1_000.0),
        Some(v) => format!("{v:.0} B/s"),
        None => "—".into(),
    }
}
fn meter(n: Option<f64>, width: u16) -> Line<'static> {
    thin_meter(n, width)
}
fn graph(history: &[u64], width: u16, color: Color) -> Line<'static> {
    if history.is_empty() {
        text("— history unavailable", DIM)
    } else {
        text(spark(history, usize::from(width)), color)
    }
}
fn interface(m: &metrics::Snapshot, selected: usize) -> (&str, Option<f64>, Option<f64>) {
    let index = selected % (m.details.interfaces.len() + 1);
    if index == 0 {
        ("all non-loopback", m.rx, m.tx)
    } else {
        let (name, rx, tx) = &m.details.interfaces[index - 1];
        (name, *rx, *tx)
    }
}
fn panel_lines(i: usize, m: &metrics::Snapshot, s: &State, width: u16) -> Vec<Line<'static>> {
    let d = &m.details;
    let mut lines = Vec::new();
    match i {
        0 => {
            lines.push(value(percent(m.cpu), GREEN));
            lines.push(meter(m.cpu, width));
            lines.push(text(
                graph_history(
                    &d.cpu_history,
                    usize::from(width.min(24)),
                    GraphScale::Percent,
                ),
                GREEN,
            ));
            lines.push(text(
                format!("Load 1 / 5 / 15m: {}", d.load.as_deref().unwrap_or("—")),
                TEXT,
            ));
            lines.push(text(
                format!(
                    "Up {}",
                    d.uptime
                        .map(|n| format!("{:.1} days", n / 86400.0))
                        .unwrap_or_else(|| "—".into())
                ),
                TEXT,
            ));
            if d.cores.is_empty() {
                lines.push(text("Per-core usage unavailable", DIM));
            } else {
                // Pack cores across each row so large machines need less scrolling.
                let count = (usize::from(width) / 16).max(1);
                for chunk in d.cores.iter().enumerate().collect::<Vec<_>>().chunks(count) {
                    lines.push(Line::from(
                        chunk
                            .iter()
                            .map(|(n, v)| {
                                Span::styled(
                                    format!("cpu{n:02} {:>6}   ", percent(Some(**v))),
                                    Style::default().fg(TEXT),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ));
                }
            }
        }
        1 => {
            let used = d
                .mem_total
                .zip(d.mem_available)
                .map(|(t, a)| t.saturating_sub(a));
            lines.push(value(
                format!("{} / {}", size(used), size(d.mem_total)),
                GREEN,
            ));
            lines.push(meter(m.mem, width));
            lines.push(text(
                format!(
                    "Cache {} · available {}",
                    size(d.mem_cached),
                    size(d.mem_available)
                ),
                TEXT,
            ));
            lines.push(text(
                format!(
                    "Swap {} / {}",
                    size(
                        d.swap_total
                            .zip(d.swap_free)
                            .map(|(t, f)| t.saturating_sub(f))
                    ),
                    size(d.swap_total)
                ),
                TEXT,
            ));
            lines.push(meter(
                d.swap_total.zip(d.swap_free).map(|(t, f)| {
                    if t == 0 {
                        0.0
                    } else {
                        t.saturating_sub(f) as f64 / t as f64 * 100.0
                    }
                }),
                width,
            ));
            lines.push(text(
                format!(
                    "RTT {}",
                    m.rtt
                        .map(|v| format!("{v:.0}ms"))
                        .unwrap_or_else(|| "—".into())
                ),
                MID,
            ));
            lines.push(text(
                graph_history(
                    &m.rtt_history,
                    usize::from(width.min(16)),
                    GraphScale::Latency,
                ),
                GREEN,
            ));
        }
        2 => {
            let (name, rx, tx) = interface(m, s.interface);
            let history = d.interface_history.get(name);
            let aggregate = s.interface.is_multiple_of(d.interfaces.len() + 1);
            let rx_history = if aggregate {
                m.rx_history.as_slice()
            } else {
                history.map(|h| h.0.as_slice()).unwrap_or(&[])
            };
            let tx_history = if aggregate {
                m.tx_history.as_slice()
            } else {
                history.map(|h| h.1.as_slice()).unwrap_or(&[])
            };
            lines.push(value(format!("↓ RX  {}", rate(rx)), GREEN));
            lines.push(graph(
                rx_history,
                width.min(if s.half { 26 } else { 44 }),
                GREEN,
            ));
            lines.push(value(format!("↑ TX  {}", rate(tx)), CYAN));
            lines.push(graph(
                tx_history,
                width.min(if s.half { 26 } else { 44 }),
                CYAN,
            ));
            lines.push(text(
                format!(
                    "TCP latency {} · i Interface",
                    m.rtt
                        .map(|v| format!("{v:.0}ms"))
                        .unwrap_or_else(|| "—".into())
                ),
                MID,
            ));
        }
        3 => {
            if d.disks.is_empty() {
                lines.push(text("Filesystem data unavailable", DIM));
            }
            for disk in &d.disks {
                lines.push(text(
                    format!(
                        "{}  {} / {}  {}",
                        disk.mount,
                        size(Some(disk.used_bytes / 1024)),
                        size(Some(disk.total_bytes / 1024)),
                        percent(Some(disk.use_pct))
                    ),
                    TEXT,
                ));
                lines.push(meter(Some(disk.use_pct), width));
            }
            lines.push(text(
                format!("Physical I/O ↓ {}  ↑ {}", rate(d.io_read), rate(d.io_write)),
                MID,
            ));
        }
        4 => {
            let mut processes = d.processes.clone();
            processes.sort_by(|a, b| {
                if s.memory_sort {
                    b.mem_pct.total_cmp(&a.mem_pct)
                } else {
                    b.cpu_pct.total_cmp(&a.cpu_pct)
                }
            });
            lines.push(text("PID     COMMAND           CPU%       RSS", MID));
            if processes.is_empty() {
                lines.push(text("Process data unavailable", DIM));
            }
            for p in processes {
                lines.push(text(
                    format!(
                        "{:<7} {:<16} {:>5.1}  {:>8}",
                        p.pid,
                        p.name.chars().take(16).collect::<String>(),
                        p.cpu_pct,
                        rss(p.mem_rss_kb)
                    ),
                    TEXT,
                ));
            }
        }
        _ => {
            let peer_width = usize::from(width.saturating_sub(20)).max(8);
            lines.push(text(
                format!("{:<peer_width$} {:>8} {:>8}", "PEER", "RX/s", "TX/s"),
                MID,
            ));
            if m.peers.is_empty() {
                lines.push(text("No TCP peer counters available", DIM));
            }
            for (peer, rx, tx) in &m.peers {
                // Put a long IPv6 endpoint on its own line, keeping rates aligned.
                if peer.chars().count() > peer_width {
                    lines.push(text(peer, TEXT));
                    lines.push(text(
                        format!(
                            "{:<peer_width$} {:>8} {:>8}",
                            "",
                            metrics::rate(Some(*rx)),
                            metrics::rate(Some(*tx))
                        ),
                        TEXT,
                    ));
                } else {
                    lines.push(text(
                        format!(
                            "{peer:<peer_width$} {:>8} {:>8}",
                            metrics::rate(Some(*rx)),
                            metrics::rate(Some(*tx))
                        ),
                        TEXT,
                    ));
                }
            }
        }
    }
    lines
}

pub(super) fn draw_health(
    f: &mut Frame,
    r: Rect,
    sample: Option<&metrics::Snapshot>,
    s: &mut State,
) {
    let header = Rect::new(r.x, r.y, r.width, 2.min(r.height));
    // Keep the panel structure visible while connecting or waiting for metrics.
    let empty = metrics::Snapshot::default();
    let m = sample.unwrap_or(&empty);
    let title = if sample.is_none() {
        "Collecting health… Requires a connected SSH session. F4 Main".into()
    } else {
        format!(
            "{} │ uptime {} │ sample {}s ago{}",
            if m.details.os.is_empty() {
                "OS unavailable"
            } else {
                &m.details.os
            },
            m.details
                .uptime
                .map(|n| format!("{:.1}h", n / 3600.0))
                .unwrap_or_else(|| "—".into()),
            chrono::Utc::now().timestamp().saturating_sub(m.at).max(0),
            if m.fresh() {
                ""
            } else {
                " · STALE / disconnected"
            }
        )
    };
    line(f, header, title, MID);
    let body = Rect::new(
        r.x,
        r.y + header.height,
        r.width,
        r.height.saturating_sub(header.height),
    );
    let mut panels = Vec::new();
    if body.width >= if s.half { 60 } else { 100 } && body.height >= if s.half { 32 } else { 26 } {
        let rows = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(if s.half { 18 } else { 12 }),
            Constraint::Length(7),
        ])
        .spacing(1)
        .flex(ratatui::layout::Flex::Start)
        .split(body);
        panels.push((2, rows[0]));
        if s.half {
            let middle = Layout::horizontal([Constraint::Ratio(1, 2); 2])
                .spacing(2)
                .split(rows[1]);
            let left = Layout::vertical([Constraint::Ratio(1, 2); 2]).split(middle[0]);
            panels.extend([(0, left[0]), (1, left[1]), (3, middle[1])]);
        } else {
            let middle = Layout::horizontal([Constraint::Ratio(1, 3); 3])
                .spacing(2)
                .split(rows[1]);
            panels.extend([(0, middle[0]), (1, middle[1]), (3, middle[2])]);
        }
        let bottom = Layout::horizontal([Constraint::Ratio(1, 2); 2])
            .spacing(2)
            .split(rows[2]);
        panels.extend([(5, bottom[0]), (4, bottom[1])]);
    } else {
        let mut x = body.x;
        for (i, name) in PANELS.iter().enumerate() {
            let width = (name.len() as u16 + 3).min(body.right().saturating_sub(x));
            let rect = Rect::new(x, body.y, width, 1.min(body.height));
            line(
                f,
                rect,
                format!(" {name} "),
                if s.focus == i { CYAN } else { DIM },
            );
            s.hits.push((rect, Click::Panel(i)));
            x += width;
        }
        let tabs_height = 1.min(body.height);
        panels.push((
            s.focus,
            Rect::new(
                body.x,
                body.y + tabs_height,
                body.width,
                body.height.saturating_sub(tabs_height),
            ),
        ));
    }
    for (i, rect) in &panels {
        let i = *i;
        s.hits.push((*rect, Click::Panel(i)));
        let title = match i {
            2 => format!("Network · {}", interface(m, s.interface).0),
            4 => format!(
                "Processes · {} sort",
                if s.memory_sort { "Memory" } else { "CPU" }
            ),
            _ => PANELS[i].into(),
        };
        let block = Block::default()
            .borders(Borders::TOP)
            .title(Span::styled(
                format!(" {title} "),
                Style::default().fg(CYAN).bold(),
            ))
            .border_style(Style::default().fg(if s.focus == i { CYAN } else { BORDER }));
        let inner = block.inner(*rect).inner(Margin::new(1, 0));
        f.render_widget(block, *rect);
        let hint = match i {
            4 => "c CPU sort · m Memory sort · ↑↓ Scroll",
            5 => "TCP deltas; short-lived sockets may be missed",
            _ => "",
        };
        let hint_height = u16::from(!hint.is_empty() && inner.height >= 3);
        let content = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(hint_height),
        );
        let lines = panel_lines(i, m, s, content.width);
        if i == 2 && content.width >= 60 {
            let cols = Layout::horizontal([Constraint::Ratio(1, 2); 2])
                .spacing(3)
                .split(content);
            for (col, start) in cols.iter().zip([0, 2]) {
                let rows = panel_lines(i, m, s, col.width);
                f.render_widget(Paragraph::new(rows[start..start + 2].to_vec()), *col);
            }
            continue;
        }
        // Keep tabular rows aligned; clamp scrolling when data or terminal size changes.
        let count = lines.len().min(u16::MAX as usize) as u16;
        let paragraph = Paragraph::new(lines);
        s.scroll[i] = s.scroll[i].min(count.saturating_sub(content.height));
        f.render_widget(paragraph.scroll((s.scroll[i], 0)), content);
        if hint_height > 0 {
            line(
                f,
                Rect::new(inner.x, content.bottom(), inner.width, 1),
                hint,
                DIM,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn sample() -> metrics::Snapshot {
        metrics::Snapshot {
            at: chrono::Utc::now().timestamp(),
            cpu: Some(23.4),
            mem: Some(45.0),
            rx: Some(8_400_000.0),
            tx: Some(1_200_000.0),
            rtt: Some(4.0),
            rx_history: vec![1, 3, 5, 8, 6, 3, 2, 4, 5, 7],
            tx_history: vec![1, 1, 2, 3, 1, 1, 2, 4, 2, 1],
            peers: vec![("10.0.1.14:5432".into(), 5_900_000.0, 412_000.0)],
            details: super::super::super::super::details::Details {
                os: "Linux 6.16".into(),
                uptime: Some(44280.0),
                load: Some("0.62 0.48 0.31".into()),
                mem_total: Some(8 * 1048576),
                mem_available: Some(4613734),
                swap_total: Some(2 * 1048576),
                swap_free: Some(2 * 1048576),
                cores: vec![32.0, 18.0, 26.0, 17.0],
                cpu_history: vec![5, 8, 12, 23, 32, 23, 18, 20, 23],
                mem_history: vec![42, 43, 43, 44, 45, 45, 45],
                disks: vec![crate::monitor::DiskInfo {
                    mount: "/".into(),
                    total_bytes: 80 * 1024 * 1048576,
                    used_bytes: 19 * 1024 * 1048576,
                    use_pct: 23.75,
                }],
                processes: vec![crate::monitor::ProcessInfo {
                    pid: 1842,
                    name: "postgres".into(),
                    cpu_pct: 18.2,
                    mem_pct: 8.4,
                    ..Default::default()
                }],
                io_read: Some(1_200_000.0),
                io_write: Some(420_000.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn dashboard_matches_reference_hierarchy_and_colors() {
        let m = sample();
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut s = State::default();
        term.draw(|f| {
            background(f);
            draw_health(f, f.area(), Some(&m), &mut s);
        })
        .unwrap();
        let buffer = term.backend().buffer();
        let content = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        for expected in [
            "23.4%",
            "8.4 MB/s",
            "1.2 MB/s",
            "postgres",
            "10.0.1.14:5432",
            "CPU sort",
        ] {
            assert!(content.contains(expected), "missing {expected}");
        }
        for (symbol, color) in [("↓", GREEN), ("↑", CYAN), ("─", GREEN)] {
            assert!(buffer
                .content
                .iter()
                .any(|c| c.symbol() == symbol && c.fg == color));
        }
        let panels: Vec<_> = s
            .hits
            .iter()
            .filter_map(|(r, c)| {
                if let Click::Panel(i) = c {
                    Some((*i, *r))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(panels.len(), 6);
        let rect = |id| panels.iter().find(|(i, _)| *i == id).unwrap().1;
        assert!(rect(2).bottom() <= rect(0).y);
        assert_eq!(rect(0).y, rect(1).y);
        assert_eq!(rect(1).y, rect(3).y);
        assert_eq!(rect(5).y, rect(4).y);

        // Optional artifact for visually reviewing the real ratatui renderer.
        if let Ok(path) = std::env::var("ESSH_HEALTH_RENDER") {
            let cells:Vec<_> = buffer.content.iter().map(|c| serde_json::json!({"text":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg)})).collect();
            fs::write(
                path,
                serde_json::to_vec(&serde_json::json!({"width":120,"height":40,"cells":cells}))
                    .unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn missing_metrics_keep_panels_and_scrolling_recovers() {
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        let mut s = State {
            focus: 1,
            scroll: [100; 6],
            ..Default::default()
        };
        term.draw(|f| draw_health(f, f.area(), None, &mut s))
            .unwrap();
        let content = term
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(content.contains("Collecting health"));
        assert!(content.contains("usage unavailable"));
        assert!(!content.contains("0.0%"));
        assert_eq!(s.scroll[1], 0);
    }
}
