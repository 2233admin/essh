use ratatui::{
    prelude::*,
    widgets::{Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::design as d;
use crate::portfwd::{ForwardState, PortForwardManager};
use crate::theme::Theme;

/// Width the panel wants. The add-form's format line is the widest thing it
/// has to say, and v0.3.3 fixed the popup at 65 columns and then printed a
/// 78-column hint into it — the format ran off the panel edge and stopped
/// mid-token, so the one line telling you what to type was the line you could
/// not read.
const WANTED_WIDTH: u16 = 100;

pub fn render(
    f: &mut Frame,
    manager: &PortForwardManager,
    host: &str,
    input: &str,
    adding: bool,
    theme: &Theme,
) {
    let area = f.area();

    let popup_width = WANTED_WIDTH.min(area.width.saturating_sub(4));
    // The add form needs three rows of its own: format, input, and the blank
    // between them. Asking for them up front is what keeps the hint inside.
    let form_rows: u16 = if adding { 4 } else { 0 };
    let rows = manager.forwards.len().max(1) as u16;
    let popup_height = (rows + form_rows + 5)
        .min(area.height.saturating_sub(2))
        .max(5);
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup);

    let active = manager
        .forwards
        .iter()
        .filter(|f| f.state.is_active())
        .count();
    // SPEC §3.4: whose forwards these are, and how many rules there are.
    // "2 active" on a panel that could belong to any of nine sessions is a
    // count without a subject.
    let sub = format!(
        "{host} · {} · {active} active",
        crate::format::count(manager.forwards.len(), "rule")
    );

    let inner = d::pane(
        f,
        popup,
        theme,
        &d::PanelOpts {
            title: Some("port forwards"),
            sub: (!manager.is_empty()).then_some(sub.as_str()),
            foot_left: &[
                ("↑↓", " select"),
                ("a", " add"),
                ("r", " retry"),
                ("d", " delete"),
                ("⎋", " close"),
            ],
            ..Default::default()
        },
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(form_rows)])
        .split(inner);

    if manager.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled(
                "  No port forwards. Press a to add one.",
                Style::default().fg(theme.text_muted),
            )),
            chunks[0],
        );
    } else {
        render_table(f, chunks[0], manager, theme);
    }

    if adding {
        render_add_form(f, chunks[1], input, theme);
    }
}

fn render_table(f: &mut Frame, area: Rect, manager: &PortForwardManager, theme: &Theme) {
    let header = Row::new(vec![
        Cell::from(d::header_cell("dir", false, theme)),
        Cell::from(d::header_cell("bind", false, theme)),
        Cell::from(d::header_cell("target", false, theme)),
        Cell::from(d::header_cell("state", false, theme)),
        Cell::from(d::header_cell("detail", false, theme)),
    ]);

    let rows: Vec<Row> = manager
        .forwards
        .iter()
        .enumerate()
        .map(|(i, fwd)| {
            let selected = i == manager.selected;
            let name_style = if selected {
                Style::default().fg(theme.text_emphasis).bold()
            } else {
                Style::default().fg(theme.text_primary)
            };

            // Colour follows the evidence: green only when a listener exists.
            //
            // SPEC §3.4's LAST CHECK, said honestly. The reference calls it a
            // probe time; nothing probes, because we own the listener and the
            // bind result is the state (SPEC §6.3). So it reports when that
            // result was established, in those words.
            // `bound now ago` is not English. The zero case gets its own
            // words rather than a unit nobody would say out loud.
            let elapsed = fwd.state_since.elapsed().as_secs();
            let since = if elapsed < 3 {
                "just now".to_string()
            } else {
                format!("{} ago", crate::format::short_duration(elapsed))
            };
            let (state_colour, detail) = match &fwd.state {
                ForwardState::Active { connections } => (
                    theme.status_good,
                    match connections {
                        0 => format!("bound {since} · no connections yet"),
                        n => format!(
                            "bound {since} · {}",
                            crate::format::count(*n as usize, "open connection")
                        ),
                    },
                ),
                // The retry key lives on the panel's bottom rule. Repeating
                // it in the cell is what pushed the reason past the edge.
                ForwardState::Failed { reason } => {
                    (theme.status_error, format!("{reason} · {since}"))
                }
                ForwardState::Starting => (theme.status_warn, "binding…".to_string()),
            };

            Row::new(vec![
                Cell::from(fwd.direction.to_string()).style(Style::default().fg(theme.text_muted)),
                Cell::from(format!("{}:{}", fwd.bind_host, fwd.bind_port)).style(name_style),
                Cell::from(format!("{}:{}", fwd.target_host, fwd.target_port))
                    .style(Style::default().fg(theme.text_secondary)),
                Cell::from(Line::from(vec![
                    d::dot(state_colour),
                    Span::styled(
                        format!(" {}", fwd.state.label()),
                        Style::default().fg(state_colour),
                    ),
                ])),
                Cell::from(detail).style(Style::default().fg(theme.text_muted)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Length(22),
        Constraint::Length(22),
        Constraint::Length(10),
        Constraint::Min(20),
    ];
    f.render_widget(Table::new(rows, widths).header(header), area);
}

/// The add form, laid out so the format line has a row to itself.
fn render_add_form(f: &mut Frame, area: Rect, input: &str, theme: &Theme) {
    let dim = Style::default().fg(theme.text_muted);
    let key = Style::default().fg(theme.key_hint);

    let lines = vec![
        Line::from(vec![
            Span::styled("  L ", key),
            Span::styled("127.0.0.1:PORT → HOST:PORT", dim),
            Span::styled("   local, and the only one implemented", dim),
        ]),
        Line::from(vec![
            Span::styled("  ❯ ", key),
            Span::styled(input.to_string(), Style::default().fg(theme.text_primary)),
            Span::styled("█", Style::default().fg(theme.brand)),
        ]),
        Line::from(vec![
            Span::styled("  ⏎", key),
            Span::styled(" save   ", dim),
            Span::styled("⎋", key),
            Span::styled(" cancel", dim),
        ]),
    ];

    // Wrapped rather than clipped: if a terminal is narrow enough that the
    // format still does not fit, it continues on the next row instead of
    // stopping mid-token.
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfwd::PortForwardManager;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen(width: u16, height: u16, manager: &PortForwardManager, adding: bool) -> String {
        let theme = crate::theme::dark();
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| render(f, manager, "web-02", "8080:localhost:80", adding, &theme))
            .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn manager_with(state: ForwardState) -> PortForwardManager {
        let mut mgr = PortForwardManager::new();
        let id = mgr.add_local("127.0.0.1", 8080, "localhost", 80).id.clone();
        mgr.set_state(&id, state);
        mgr
    }

    /// The clipped hint. v0.3.3 fixed the popup at 65 columns and printed a
    /// longer format string into it, so the one line that tells you what to
    /// type ended mid-token at the panel edge.
    #[test]
    fn the_add_form_format_is_never_clipped() {
        for width in [70, 90, 120, 150] {
            let s = screen(width, 24, &PortForwardManager::new(), true);
            assert!(
                s.contains("127.0.0.1:PORT → HOST:PORT"),
                "at {width} cols the format hint is cut off:\n{s}"
            );
            assert!(
                s.contains("save") && s.contains("cancel"),
                "at {width} cols the form lost its keys:\n{s}"
            );
        }
    }

    /// The reported bug: two forwards listed as "Active" with nothing bound.
    #[test]
    fn a_forward_that_did_not_bind_is_never_shown_as_active() {
        let mgr = manager_with(ForwardState::Failed {
            reason: "port 8080 already in use".into(),
        });
        let s = screen(120, 16, &mgr, false);

        assert!(s.contains("● failed"), "state not shown:\n{s}");
        assert!(
            !s.contains("● active"),
            "a failed forward claimed active:\n{s}"
        );
        assert!(
            s.contains("1 rule") && s.contains("0 active"),
            "the panel should name the host and count what is bound:\n{s}"
        );
        assert!(
            s.contains("web-02"),
            "the panel does not say whose forwards these are:\n{s}"
        );
        // In full, not cut off at the panel edge: a clipped reason is the
        // add-forward hint's bug wearing a different hat.
        assert!(
            s.contains("port 8080 already in use"),
            "the reason is missing or clipped:\n{s}"
        );
        assert!(s.contains("r retry"), "no way offered to try again:\n{s}");
    }

    /// A forward that has been asked for but not yet bound is neither.
    #[test]
    fn a_starting_forward_claims_nothing() {
        let mgr = manager_with(ForwardState::Starting);
        let s = screen(120, 16, &mgr, false);
        assert!(s.contains("● starting"), "{s}");
        assert!(!s.contains("● active"), "{s}");
        assert!(!s.contains("● failed"), "{s}");
    }

    #[test]
    fn an_active_forward_reports_its_connections_from_the_count() {
        let mgr = manager_with(ForwardState::Active { connections: 1 });
        let s = screen(120, 16, &mgr, false);
        assert!(s.contains("● active"), "{s}");
        assert!(
            s.contains("1 open connection"),
            "singular count, and from the counter:\n{s}"
        );
        assert!(!s.contains("1 open connections"), "{s}");
    }

    #[test]
    fn the_panel_renders_at_every_size_without_panicking() {
        let mgr = manager_with(ForwardState::Active { connections: 0 });
        for (w, h) in [(20, 6), (40, 10), (80, 24), (150, 34), (200, 60)] {
            let _ = screen(w, h, &mgr, true);
            let _ = screen(w, h, &mgr, false);
        }
    }

    /// Print the panel rather than assert on it, the way `dump_every_command`
    /// does for the session screens.
    /// `cargo test --bins portfwd_view::tests::visual_dump -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn visual_dump() {
        let mut mgr = PortForwardManager::new();
        let a = mgr.add_local("127.0.0.1", 8080, "localhost", 80).id.clone();
        let b = mgr
            .add_local("127.0.0.1", 15432, "localhost", 5432)
            .id
            .clone();
        mgr.set_state(&a, ForwardState::Active { connections: 1 });
        mgr.set_state(
            &b,
            ForwardState::Failed {
                reason: "port 15432 already in use".into(),
            },
        );
        println!("\n--- idle ---\n{}", screen(150, 16, &mgr, false));
        println!("\n--- adding ---\n{}", screen(150, 20, &mgr, true));
    }
}
