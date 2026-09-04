use crate::design as d;
use crate::session::{Session, SessionState};
use crate::theme::Theme;
use crate::tui::widgets;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};

/// Render the dashboard view (no active session focused)
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    sessions: &[Session],
    hosts: &[super::HostDisplay],
    filtered_indices: &[usize],
    selected_host: usize,
    selected_session: usize,
    table_state: &mut TableState,
    active_tab: super::DashboardTab,
    status_message: Option<&str>,
    search_active: bool,
    search_query: &str,
    groups: &[crate::divergence::GroupSummary],
    fleet_consensus: Option<(String, crate::divergence::Consensus)>,
    facet_agreement: &[(String, f64)],
    verdicts: &[(String, crate::divergence::Verdict)],
    theme: &Theme,
) {
    // The footer is a bordered block, so its height must account for every
    // line it will hold. Without the status row, `set_status` output was
    // laid out past the bottom edge and silently never rendered — which is
    // every error and result message in the app, not just one.
    // The footer is plain text now — the design gives it a rule above, not a
    // box around. Height is exactly the lines it will hold, so a status
    // message is never laid out past the bottom edge.
    let mut footer_height = 1;
    if search_active {
        footer_height += 1;
    }
    if status_message.is_some() {
        footer_height += 1;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // one line of chrome
            Constraint::Min(8),                // main content
            Constraint::Length(footer_height), // footer (+ search bar)
        ])
        .split(area);

    d::paint_bg(f, area, theme);
    render_header(
        f,
        chunks[0],
        super::AppView::Dashboard,
        active_tab,
        hosts,
        sessions.len(),
        theme,
    );

    match active_tab {
        super::DashboardTab::Sessions => {
            render_sessions_tab(f, chunks[1], sessions, selected_session, theme)
        }
        super::DashboardTab::Hosts => {
            // The dead space under a short host list becomes the GROUPS panel:
            // which peer sets exist, whether they agree, and who breaks them.
            let group_height = if groups.is_empty() {
                0
            } else {
                (groups.len() as u16 + 3).min(chunks[1].height / 2)
            };
            // Sized to content. A box stretched over empty space reads as
            // missing data rather than as an absence of problems.
            let host_height = ((filtered_indices.len() as u16) + 3)
                .max(5)
                .min(chunks[1].height.saturating_sub(group_height));
            let host_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(host_height),
                    Constraint::Length(group_height),
                    Constraint::Min(0),
                ])
                .split(chunks[1]);

            render_hosts_tab(
                f,
                host_chunks[0],
                hosts,
                filtered_indices,
                selected_host,
                table_state,
                theme,
            );
            if group_height > 0 {
                render_groups_panel(f, host_chunks[1], groups, theme);
            }
        }
        super::DashboardTab::Fleet => render_fleet_tab(
            f,
            chunks[1],
            hosts,
            sessions,
            fleet_consensus,
            facet_agreement,
            verdicts,
            selected_host,
            theme,
        ),
        super::DashboardTab::Config => render_config_tab(f, chunks[1], theme),
    }

    render_footer(
        f,
        chunks[2],
        active_tab,
        status_message,
        search_active,
        search_query,
        theme,
    );
}

/// The titlebar and tab strip.
///
/// The global header, one line. See `tui::chrome`.
///
/// This used to be three rows: a centred title, a tab strip, and an underline
/// row that was blank under every inactive tab. The v0.3.3 screen tour counted
/// the dead band it left among the defects worth fixing, and the redesign's
/// answer is that an active tab can be marked by weight and a background for
/// no rows at all.
#[allow(clippy::too_many_arguments)]
fn render_header(
    f: &mut Frame,
    area: Rect,
    view: super::AppView,
    active_tab: super::DashboardTab,
    hosts: &[super::HostDisplay],
    session_count: usize,
    theme: &Theme,
) {
    let line = super::chrome::header(view, active_tab, hosts, session_count, area.width, theme);
    f.render_widget(Paragraph::new(line), area);
}

fn render_sessions_tab(
    f: &mut Frame,
    area: Rect,
    sessions: &[Session],
    selected: usize,
    theme: &Theme,
) {
    if sessions.is_empty() {
        let msg = Paragraph::new(vec![
            Line::raw(""),
            Line::styled(
                "  No active sessions.",
                Style::default().fg(theme.text_muted),
            ),
            Line::raw(""),
            Line::styled(
                "  Press [2] to browse hosts, or use 'essh connect <host>' to start a session.",
                Style::default().fg(theme.text_muted),
            ),
        ])
        .block(
            Block::bordered()
                .title("Active Sessions")
                .border_style(Style::default().fg(theme.border)),
        );
        f.render_widget(msg, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from(" # ").style(Style::default().fg(theme.brand).bold()),
        Cell::from("Label").style(Style::default().fg(theme.brand).bold()),
        Cell::from("Host").style(Style::default().fg(theme.brand).bold()),
        Cell::from("User").style(Style::default().fg(theme.brand).bold()),
        Cell::from("Status").style(Style::default().fg(theme.brand).bold()),
        Cell::from("Uptime").style(Style::default().fg(theme.brand).bold()),
    ])
    .height(1);

    let rows: Vec<Row> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let state_style = match &s.state {
                SessionState::Active => Style::default().fg(theme.status_good),
                SessionState::Suspended => Style::default().fg(theme.text_muted),
                SessionState::Reconnecting { .. } => Style::default().fg(theme.status_error),
                SessionState::Connecting => Style::default().fg(theme.status_warn),
                SessionState::Disconnected { .. } => Style::default().fg(theme.status_error),
            };
            let status_text = match &s.state {
                SessionState::Active => "● Active",
                SessionState::Suspended => "● Suspended",
                SessionState::Reconnecting { .. } => "● Recon.",
                SessionState::Connecting => "● Connecting",
                SessionState::Disconnected { .. } => "● Disconnected",
            };

            Row::new(vec![
                Cell::from(format!(" {} ", i + 1)),
                Cell::from(s.label.clone()),
                Cell::from(s.hostname.clone()),
                Cell::from(s.username.clone()),
                Cell::from(status_text).style(state_style),
                Cell::from(widgets::format_duration_short(s.uptime_secs() as i64)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Percentage(20),
        Constraint::Percentage(25),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
    ];

    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("sessions"),
            sub: Some(&format!("{}", sessions.len())),
            foot_left: &[("↑↓", " select"), ("↵", " attach"), ("w", " close")],
            ..Default::default()
        },
    );

    let table = Table::new(rows, widths)
        .header(header)
        // Background and weight only. Setting a foreground here would repaint
        // the whole row one colour, so the selected session would lose the
        // status colour that says whether it is alive — the row you are
        // looking at becomes the one row you cannot read the state of.
        .row_highlight_style(Style::default().bg(theme.selection_bg).bold())
        .highlight_symbol("▌ ");

    // Stateful, or the highlight is configured and never applied — which is
    // why this list had no visible cursor at all.
    let mut state = ratatui::widgets::TableState::default();
    state.select(Some(selected.min(sessions.len().saturating_sub(1))));
    f.render_stateful_widget(table, inner, &mut state);
}

/// The Fleet screen's reasoning block.
///
/// A count is not an insight: "3 hosts diverge" says something is wrong
/// without saying what. This names the host, the facets, and — when the
/// outliers are concentrated in a few hosts — says so, because "two hosts
/// hold every outlier" is a different operational problem from "everything
/// drifted a little".
/// Display width, so a CJK hostname or a box-drawing glyph counts for what it
/// actually occupies rather than for one column.
fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

/// Cut to `room` display columns, marking the cut. Returns the input whole
/// when it already fits, and an empty string when there is no room to say
/// anything — never a bare ellipsis taking a column it cannot justify.
fn truncate_with_ellipsis(s: &str, room: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if display_width(s) <= room {
        return s.to_string();
    }
    if room <= 1 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > room - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    // Do not leave the cut mid-word if a word boundary is close behind it.
    if let Some(i) = out.rfind(' ') {
        if display_width(&out[..i]) + 8 >= w {
            out.truncate(i);
        }
    }
    out.push('…');
    out
}

fn render_verdict_box(
    f: &mut Frame,
    area: Rect,
    verdicts: &[(String, crate::divergence::Verdict)],
    comparable: usize,
    unprobed: usize,
    theme: &Theme,
) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            title: Some("verdict"),
            right: Some("what differs, and likely why"),
            ..Default::default()
        },
    );

    let mut lines: Vec<Line> = Vec::new();

    // Only claim concentration when it is true and worth saying — and count
    // it against the hosts that were actually compared. Counting every host
    // made "2 of 10 hold every outlier" out of a fleet where seven were never
    // probed, which reads as eight hosts being fine. Unprobed is unknown, not
    // agreement (SPEC.md §1), so it is stated separately.
    if verdicts.len() >= 2 && comparable > verdicts.len() {
        let mut spans = vec![Span::styled(
            format!(
                "{} of {} comparable hosts hold every outlier.",
                verdicts.len(),
                comparable
            ),
            Style::default()
                .fg(theme.text_primary)
                .add_modifier(Modifier::BOLD),
        )];
        if unprobed > 0 {
            spans.push(Span::styled(
                format!(
                    "  {} unprobed, excluded",
                    crate::format::count(unprobed, "host")
                ),
                Style::default().fg(theme.text_muted),
            ));
        }
        lines.push(Line::from(spans));
    }

    for (host, v) in verdicts.iter().take(inner.height as usize) {
        let prefix = format!("{host}  ");
        let evidence = if v.evidence.is_empty() {
            String::new()
        } else {
            // The evidence keeps the sentence checkable.
            let facets: Vec<String> = v.evidence.iter().map(|e| e.label().to_string()).collect();
            format!("  [{}]", facets.join(", "))
        };

        // One verdict per row, so a long sentence has to be cut — but cut
        // visibly. `Paragraph` clips at the box edge with no mark at all,
        // which turned "ESSH does not infer a shared cause" into "ESSH does
        // not inf" and read as a rendering fault rather than as more text.
        // The full sentence is a keypress away in the detail card.
        let room = (inner.width as usize)
            .saturating_sub(display_width(&prefix) + display_width(&evidence));
        let text = truncate_with_ellipsis(&v.text, room);

        let mut spans = vec![
            Span::styled(
                prefix,
                Style::default()
                    .fg(theme.brand)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(text, Style::default().fg(theme.text_primary)),
        ];
        if !evidence.is_empty() {
            spans.push(Span::styled(evidence, Style::default().fg(theme.border)));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// Screen 1 — Hosts.
fn render_hosts_tab(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    filtered_indices: &[usize],
    selected: usize,
    _table_state: &mut TableState,
    theme: &Theme,
) {
    let refreshed = hosts
        .iter()
        .filter_map(|h| h.vitals.sampled_at)
        .max()
        .map(|at| format!("refreshed {}s ago", at.elapsed().as_secs()))
        .unwrap_or_else(|| "no readings yet".to_string());
    let sub = format!(
        "{} · {}",
        crate::format::count(hosts.len(), "host"),
        refreshed
    );
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("hosts"),
            sub: Some(&sub),
            foot_left: &[
                ("↑↓", " select"),
                ("⏎", " connect"),
                ("r", " reconnect"),
                ("a", " add"),
                ("e", " edit"),
                ("/", " search"),
            ],
            ..Default::default()
        },
    );
    if inner.width < 40 || inner.height < 2 {
        return;
    }

    let filtered: Vec<&super::HostDisplay> = filtered_indices
        .iter()
        .filter_map(|&i| hosts.get(i))
        .collect();

    // SPEC §3.2's budget, in cells, against a 146-cell inner width at 150
    // columns. CPU is 12 and MEM and DISK are 13 against the handoff's 8, 9,
    // 9: a meter column has to carry a 4-cell percentage as well, and what is
    // left of `8ch` after that is three cells, which cannot express a ramp.
    let cols = HostCols::fit(inner.width as usize);

    let mut lines: Vec<Line> = Vec::with_capacity(filtered.len() + 1);
    lines.push(Line::from(cols.header(theme)));

    for h in &filtered {
        let is_selected = hosts
            .get(selected)
            .is_some_and(|s| s.hostname == h.hostname && s.port == h.port);
        lines.push(
            Line::from(cols.row(h, is_selected, theme)).style(if is_selected {
                Style::default().bg(theme.selection_bg)
            } else {
                Style::default()
            }),
        );
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// The hosts table's column budget. SPEC §3.2.
///
/// Held as a struct rather than an array of `Constraint` because the row is
/// laid out by hand: a `Table` cannot span cells, and an unreachable host
/// replaces its five measurement columns with one explanation. Leaving five
/// blanks instead is what the redesign is fixing.
struct HostCols {
    name: usize,
    address: usize,
    sess: usize,
    cpu: usize,
    mem: usize,
    disk: usize,
    rtt: usize,
    spark: usize,
    tags: usize,
}

/// The gap between columns, in cells. The reference's 14px.
const GAP: usize = 2;

impl HostCols {
    /// Fit the budget to the width available, dropping columns in the order
    /// SPEC §3.6 gives rather than squeezing all of them until none is
    /// legible.
    fn fit(width: usize) -> Self {
        let mut c = HostCols {
            name: 15,
            address: 14,
            sess: 6,
            cpu: 12,
            mem: 13,
            disk: 13,
            rtt: 6,
            spark: 12,
            tags: 0,
        };
        // 1 for the state glyph, then the nine gaps.
        let fixed = |c: &HostCols| {
            1 + c.name + c.address + c.sess + c.cpu + c.mem + c.disk + c.rtt + c.spark
        };
        let gaps = |c: &HostCols| {
            GAP * (1 + [c.address, c.sess, c.cpu, c.mem, c.disk, c.rtt, c.spark]
                .iter()
                .filter(|w| **w > 0)
                .count())
        };

        // TAGS first, then the sparkline, then the meters down to bare
        // percentages, then address. Never the name or the state.
        if width > fixed(&c) + gaps(&c) {
            c.tags = width - fixed(&c) - gaps(&c);
            return c;
        }
        c.spark = 0;
        if width >= fixed(&c) + gaps(&c) {
            return c;
        }
        // A meter needs at least the btop minimum plus its percentage. Below
        // that it is a percentage on its own, which still measures something.
        c.cpu = 4;
        c.mem = 4;
        c.disk = 4;
        if width >= fixed(&c) + gaps(&c) {
            return c;
        }
        c.address = 0;
        c.mem = 0;
        c.disk = 0;
        c
    }

    fn header(&self, t: &Theme) -> Vec<Span<'static>> {
        let mut out = vec![Span::raw(" ".repeat(1 + GAP))];
        let mut push = |label: &str, w: usize, right: bool| {
            if w == 0 {
                return;
            }
            let cell = if right {
                d::fit_right(label, w, Style::default().fg(t.text_muted))
            } else {
                d::fit_left(label, w, Style::default().fg(t.text_muted))
            };
            out.extend(cell);
            out.push(Span::raw(" ".repeat(GAP)));
        };
        push("NAME", self.name, false);
        push("ADDRESS", self.address, false);
        push("SESS", self.sess, true);
        push("CPU", self.cpu, false);
        push("MEM", self.mem, false);
        push("DISK /", self.disk, false);
        push("RTT", self.rtt, true);
        push("RTT 60s", self.spark, false);
        push("TAGS", self.tags, false);
        out
    }

    /// The five measurement columns, as one span run. Used whole for a host
    /// that answers and replaced entirely for one that does not.
    fn measured_width(&self) -> usize {
        [self.cpu, self.mem, self.disk, self.rtt, self.spark]
            .iter()
            .filter(|w| **w > 0)
            .map(|w| w + GAP)
            .sum::<usize>()
            .saturating_sub(GAP)
    }

    fn row(&self, h: &super::HostDisplay, selected: bool, t: &Theme) -> Vec<Span<'static>> {
        let state = h.state();
        // SPEC §3.1: the marker replaces the dot rather than preceding it, so
        // no column shifts, and the state's colour moves onto the name.
        let mut out = vec![
            if selected {
                Span::styled("▌", Style::default().fg(t.brand))
            } else {
                Span::styled(state.glyph(), Style::default().fg(state.color(t)))
            },
            Span::raw(" ".repeat(GAP)),
        ];
        out.extend(d::fit_left(
            &h.name,
            self.name,
            Style::default().fg(if selected {
                t.text_emphasis
            } else {
                t.text_primary
            }),
        ));
        out.push(Span::raw(" ".repeat(GAP)));
        if self.address > 0 {
            out.extend(d::fit_left(
                &h.hostname,
                self.address,
                Style::default().fg(t.text_secondary),
            ));
            out.push(Span::raw(" ".repeat(GAP)));
        }
        out.extend(session_cell(h, self.sess, t));
        out.push(Span::raw(" ".repeat(GAP)));

        if state.recovery().is_some() {
            // One explanation across the measurement columns, rather than
            // five blanks and a question.
            out.extend(d::fit_left(
                &unreachable_reason(h, self.measured_width()),
                self.measured_width(),
                Style::default().fg(state.color(t)),
            ));
        } else {
            out.extend(self.measurements(h, t));
        }
        out.push(Span::raw(" ".repeat(GAP)));

        if self.tags > 0 {
            let (chips, hidden) = crate::format::tag_chips(&h.tag_pairs, self.tags);
            let mut tags: Vec<Span> = Vec::new();
            for c in &chips {
                // v2 drops `env=` and `region=` prefixes: grouping no longer
                // drives anything, so the prefix is noise in a 36-cell column.
                let flat = c.split_once('=').map(|(_, v)| v).unwrap_or(c);
                tags.push(Span::styled(
                    format!("{flat} "),
                    Style::default().fg(t.text_muted),
                ));
            }
            if hidden > 0 {
                tags.push(Span::styled(
                    format!("+{hidden}"),
                    Style::default().fg(t.border),
                ));
            }
            out.extend(d::pad_to(tags, self.tags));
        }
        out
    }

    fn measurements(&self, h: &super::HostDisplay, t: &Theme) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        // SPEC §5.3: CPU is the only ramp in the row. Memory and disk are
        // flat, so the one gradient the eye finds is the one that matters.
        out.extend(meter_cell(h.vitals.cpu_pct, self.cpu, MeterKind::Cpu, t));
        out.push(Span::raw(" ".repeat(GAP)));
        if self.mem > 0 {
            out.extend(meter_cell(h.vitals.mem_pct, self.mem, MeterKind::Mem, t));
            out.push(Span::raw(" ".repeat(GAP)));
        }
        if self.disk > 0 {
            out.extend(meter_cell(h.vitals.disk_pct, self.disk, MeterKind::Disk, t));
            out.push(Span::raw(" ".repeat(GAP)));
        }
        out.extend(d::fit_right(
            &h.latency_ms
                .map(|ms| format!("{ms:.1}"))
                .unwrap_or_else(|| "—".into()),
            self.rtt,
            Style::default().fg(if h.latency_ms.is_some() {
                t.text_secondary
            } else {
                t.absent
            }),
        ));
        if self.spark > 0 {
            out.push(Span::raw(" ".repeat(GAP)));
            let ceiling = d::axis_ceiling(&h.latency_history);
            out.extend(d::fit_left(
                &d::sparkline(&h.latency_history, self.spark, ceiling),
                self.spark,
                Style::default().fg(t.muted_good),
            ));
        }
        out
    }
}

#[derive(Clone, Copy)]
enum MeterKind {
    Cpu,
    Mem,
    Disk,
}

/// A meter and its percentage, in one column. SPEC §3.2's `Fill 4` sub-grid.
fn meter_cell(pct: Option<f64>, width: usize, kind: MeterKind, t: &Theme) -> Vec<Span<'static>> {
    // `—` for a reading we do not have. Never a zero, and never an empty
    // meter, which reads as a measured 0%.
    let Some(pct) = pct else {
        return d::fit_right("—", width, Style::default().fg(t.absent));
    };
    let number = format!("{}%", pct.round() as i64);
    let severity = match kind {
        MeterKind::Cpu | MeterKind::Mem => t.bounded_bad(pct),
        MeterKind::Disk => t.bounded_bad(pct),
    };
    // 4 for the percentage, 1 for the gap between it and the meter.
    let bar = width.saturating_sub(5);
    if bar < 3 {
        return d::fit_right(&number, width, Style::default().fg(severity));
    }
    let mut out = match kind {
        MeterKind::Cpu => t.utilisation_meter(pct / 100.0, bar),
        MeterKind::Mem => d::meter_flat(pct / 100.0, bar, t.status_info, t),
        MeterKind::Disk => d::meter_flat(
            pct / 100.0,
            bar,
            if pct >= 80.0 {
                t.status_warn
            } else {
                t.muted_good
            },
            t,
        ),
    };
    out.push(Span::raw(" "));
    out.extend(d::fit_right(&number, 4, Style::default().fg(severity)));
    d::pad_to(out, width)
}

fn session_cell(h: &super::HostDisplay, width: usize, t: &Theme) -> Vec<Span<'static>> {
    let state = h.state();
    // A dropped host's session is the reason to look at this row, so its
    // count is red rather than cyan.
    let (text, style) = match (h.session_count, state) {
        (0, _) => ("·".to_string(), Style::default().fg(t.absent)),
        (n, super::HostState::Dropped) => (n.to_string(), Style::default().fg(t.status_error)),
        (n, _) => (n.to_string(), Style::default().fg(t.brand)),
    };
    d::fit_right(&text, width, style)
}

/// SPEC §3.2. Built from parts and trimmed in a fixed order, because the
/// handoff's own string is 65 cells in a 64-cell span and hand-trimming it
/// only moves the problem to the next width.
///
/// Parts 1 and 2 are never dropped: what happened, and the key that fixes it.
fn unreachable_reason(h: &super::HostDisplay, budget: usize) -> String {
    let state = h.state();
    let what = match state {
        super::HostState::Dropped => {
            format!(
                "connection reset {}",
                crate::format::relative_time(&h.last_seen)
            )
        }
        _ if h.last_seen.is_empty() => "no answer since essh started".to_string(),
        _ => format!(
            "offline since {}",
            crate::format::relative_time(&h.last_seen)
        ),
    };
    let key = state.recovery().unwrap_or("");
    let floor = format!("{what} · {key}");

    let mut extras: Vec<String> = Vec::new();
    if let (Some(cpu), Some(mem)) = (h.vitals.cpu_pct, h.vitals.mem_pct) {
        // Only a dropped host has a last reading. One that never answered has
        // nothing to show, and `last cpu 0%` would be an invention.
        if state == super::HostState::Dropped {
            extras.push(format!("last cpu {}%, mem {}%", cpu.round(), mem.round()));
        }
    }
    if state == super::HostState::Offline {
        extras.push(format!("no response on {}", h.port));
    }

    // Widest form that fits, then drop the extras in order.
    for take in (0..=extras.len()).rev() {
        let mut out = format!("{what}");
        for e in extras.iter().take(take) {
            out.push_str(" · ");
            out.push_str(e);
        }
        out.push_str(" · ");
        out.push_str(key);
        if d::width_of(&out) <= budget {
            return out;
        }
    }
    floor
}

/// The GROUPS panel — the answer to "are these machines the same?".
///
/// This is what fills the ~900px of dead space under a short host list.
fn render_groups_panel(
    f: &mut Frame,
    area: Rect,
    groups: &[crate::divergence::GroupSummary],
    theme: &Theme,
) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("2"),
            title: Some("groups"),
            right: Some("by role · tag consensus"),
            ..Default::default()
        },
    );

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(d::header_cell("group", false, theme)),
        Cell::from(Line::from(d::header_cell("hosts", false, theme)).right_aligned()),
        Cell::from(Line::from(d::header_cell("at consensus", false, theme)).right_aligned()),
        Cell::from(d::header_cell("note", false, theme)),
    ]);

    let rows: Vec<Row> = groups
        .iter()
        .take(inner.height.saturating_sub(1) as usize)
        .map(|g| {
            let probed = g.probed();
            let severity = if probed == 0 {
                0.0
            } else {
                1.0 - (g.at_consensus as f64 / probed as f64)
            };
            // A ratio is a claim that one side won. Print it only when a
            // majority exists to have won: `1 / 2` on a 1-1 split is the
            // arbitrary tie-break rendered as a finding.
            let col = if probed == 0 {
                theme.border
            } else if !g.has_majority() {
                theme.status_warn
            } else if severity > 0.0 {
                theme.ramp_divergence(severity)
            } else {
                theme.status_good
            };
            let consensus = if probed == 0 || !g.has_majority() {
                "—".to_string()
            } else {
                format!("{} / {}", g.at_consensus, probed)
            };
            Row::new(vec![
                Cell::from(Line::from(d::dot(col))),
                Cell::from(g.label.clone()).style(Style::default().fg(theme.text_emphasis)),
                Cell::from(Line::from(g.host_count.to_string()).right_aligned())
                    .style(Style::default().fg(theme.text_secondary)),
                Cell::from(Line::from(consensus).right_aligned()).style(Style::default().fg(col)),
                Cell::from(g.note()).style(if probed == 0 {
                    Style::default()
                        .fg(theme.border)
                        .add_modifier(Modifier::ITALIC)
                } else {
                    Style::default().fg(col)
                }),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(22),
        Constraint::Length(7),
        Constraint::Length(14),
        Constraint::Min(20),
    ];
    f.render_widget(Table::new(rows, widths).header(header), inner);
}

/// Screen 2 — Fleet. Facet consensus is the headline; reachability is one
/// quiet line beneath it.
#[allow(clippy::too_many_arguments)]
fn render_fleet_tab(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    sessions: &[Session],
    fleet_consensus: Option<(String, crate::divergence::Consensus)>,
    facet_agreement: &[(String, f64)],
    verdicts: &[(String, crate::divergence::Verdict)],
    selected: usize,
    theme: &Theme,
) {
    let _ = (sessions, theme);
    // Boxes are sized to their content. Stretching a box with one row in it
    // to fill the window produces a large empty rectangle, which reads as
    // missing data rather than as an absence of problems.
    let listed = hosts
        .iter()
        .filter(|h| h.diverge_count != Some(0))
        .count()
        .max(1);
    let table_height = ((listed + 4) as u16).min(area.height.saturating_sub(7));

    // The consensus box grows to fit its facet list. Fixed at six rows it
    // showed four facets and dropped the rest without saying so, which is the
    // one thing a consensus panel must never do: quietly omit the checks.
    // Headline (4) + border (2) + however many grid rows the facets need.
    let per_row = ((area.width as usize).saturating_sub(2) / 34).max(1);
    let facet_rows = facet_agreement.len().div_ceil(per_row) as u16;
    let consensus_height = (6u16 + facet_rows)
        .max(6)
        .min(area.height.saturating_sub(6));

    // The verdict block: one line of reasoning per host that has one, plus a
    // lead when the outliers are concentrated.
    let verdict_height = if verdicts.is_empty() {
        0
    } else {
        (verdicts.len() as u16 + 3).min(area.height / 3)
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(consensus_height),
            Constraint::Length(verdict_height),
            Constraint::Length(table_height),
            Constraint::Min(0),
        ])
        .split(area);

    render_consensus_box(f, rows[0], hosts, &fleet_consensus, facet_agreement, theme);
    if verdict_height > 0 {
        let unprobed = hosts.iter().filter(|h| h.diverge_count.is_none()).count();
        render_verdict_box(
            f,
            rows[1],
            verdicts,
            hosts.len() - unprobed,
            unprobed,
            theme,
        );
    }
    render_fleet_table(f, rows[2], hosts, selected, theme);
}

/// The CONSENSUS box: a 26px numeral in the design, the biggest thing a
/// terminal can offer here, plus what it actually means.
fn render_consensus_box(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    consensus: &Option<(String, crate::divergence::Consensus)>,
    facet_agreement: &[(String, f64)],
    theme: &Theme,
) {
    let right = consensus
        .as_ref()
        .map(|(label, c)| format!("{} facet-checks · {}", c.total_checks, label))
        .unwrap_or_default();
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("3"),
            title: Some("consensus"),
            right: Some(&right),
            ..Default::default()
        },
    );

    // Headline across the top, facets in a grid beneath it.
    //
    // Side-by-side wasted most of the box: the headline is four lines, so a
    // sixteen-row box left twelve rows of empty space on the left while the
    // facet list was squeezed into one narrow column with its labels
    // truncated.
    let rows_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(inner);
    let cols = [rows_split[0], rows_split[1]];

    let mut lines: Vec<Line> = Vec::new();
    match consensus {
        Some((_, c)) if c.total_checks > 0 => {
            let pct = c.percent();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:.1}%", pct),
                    Style::default()
                        .fg(theme.text_emphasis)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("of {} facet-checks agree", c.total_checks),
                    Style::default().fg(theme.text_secondary),
                ),
            ]));
            let plural = |n: usize, w: &str| {
                if n == 1 {
                    format!("{} {}", n, w)
                } else {
                    format!("{} {}s", n, w)
                }
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{} diverge across {}.",
                    plural(c.diverging_facets, "facet"),
                    plural(c.diverging_hosts, "host")
                ),
                Style::default().fg(theme.text_secondary),
            )));
            if c.unprobed_hosts > 0 {
                // The distinction v1's `Offline: 8` destroyed.
                let (n, plural) = (
                    c.unprobed_hosts,
                    if c.unprobed_hosts == 1 {
                        "host has"
                    } else {
                        "hosts have"
                    },
                );
                lines.push(d::none_line(
                    &format!(
                        "{n} {plural} never been probed, so their facets are unknown rather than in agreement."
                    ),
                    theme,
                ));
            }
        }
        _ => {
            lines.push(d::none_line(
                "no facets collected yet — connect to a host to compare it against its peers",
                theme,
            ));
        }
    }

    // Reachability, demoted to one dim line. v1 gave this a pink 20% bar
    // that "read as an alarm about nothing".
    let online = hosts
        .iter()
        .filter(|h| h.status == super::HostStatus::Online)
        .count();
    let never = hosts
        .iter()
        .filter(|h| h.status == super::HostStatus::NeverProbed)
        .count();
    let mut reach = vec![
        Span::styled("reachable ", Style::default().fg(theme.border)),
        Span::styled(
            online.to_string(),
            Style::default().fg(theme.text_secondary),
        ),
        Span::styled(" of ", Style::default().fg(theme.border)),
        Span::styled(
            hosts.len().to_string(),
            Style::default().fg(theme.text_secondary),
        ),
    ];
    if never > 0 {
        reach.push(Span::styled(
            format!(" · {} never probed", never),
            Style::default().fg(theme.border),
        ));
    }
    lines.push(Line::from(reach));
    f.render_widget(Paragraph::new(lines), cols[0]);

    // Per-facet agreement meters, so the box says *where* the fleet is
    // ragged rather than only how ragged it is.
    if !facet_agreement.is_empty() && cols[1].height > 0 {
        // The facet name is the information; the bar is the decoration. It
        // used to be the other way round — a 14-cell bar next to a name
        // truncated to 14 characters, so full bars read as one green slab
        // and the labels said "/etc/nginx/ng…".
        let meter_w = 8usize;
        let cell_w = 34usize;
        let per_row = ((cols[1].width as usize) / cell_w).max(1);
        let label_w = cell_w.saturating_sub(meter_w + 8).max(10);
        let capacity = per_row * cols[1].height as usize;
        let shown = capacity.min(facet_agreement.len());

        let cells: Vec<Vec<Span>> = facet_agreement
            .iter()
            .take(shown)
            .map(|(label, frac)| {
                let agrees = *frac >= 1.0;

                // SPEC.md §2.1: agreement needs no graphic. Dimming the bar
                // was not enough — a grid of full-width bars in adjacent rows
                // merges into one solid rectangle whatever colour it is, and
                // the facet that actually diverges is the one short bar lost
                // inside it. So a facet at 100% gets its number and nothing
                // else, and the meters that remain are all divergence.
                //
                // Severity by shortfall, not by a ramp read at `1 - frac`:
                // that put 86% agreement near the ramp's cold end, so the
                // diverging facets rendered quieter than the agreeing ones.
                let col = if agrees {
                    theme.muted_good
                } else if *frac < 0.6 {
                    theme.status_error
                } else {
                    theme.status_warn
                };

                let (fill, track) = if agrees {
                    (String::new(), " ".repeat(meter_w))
                } else {
                    d::meter(*frac, meter_w)
                };

                vec![
                    Span::styled(
                        format!(
                            "{:<w$}",
                            crate::format::truncate_right(label, label_w),
                            w = label_w
                        ),
                        // A diverging facet's name is the thing to read.
                        Style::default().fg(if agrees {
                            theme.border
                        } else {
                            theme.text_primary
                        }),
                    ),
                    Span::styled(fill, Style::default().fg(col)),
                    Span::styled(track, Style::default().fg(theme.border)),
                    Span::styled(format!("{:>4.0}% ", frac * 100.0), Style::default().fg(col)),
                    Span::raw("  "),
                ]
            })
            .collect();

        let mut lines: Vec<Line> = cells
            .chunks(per_row)
            .map(|chunk| Line::from(chunk.iter().flatten().cloned().collect::<Vec<_>>()))
            .collect();
        // Never drop checks silently: an omitted facet is an unexamined one.
        if shown < facet_agreement.len() {
            lines.push(Line::from(Span::styled(
                format!("+{} more facets", facet_agreement.len() - shown),
                Style::default().fg(theme.border),
            )));
        }
        f.render_widget(Paragraph::new(lines), cols[1]);
    }
}

/// HOSTS BY DIVERGENCE, sorted worst first.
fn render_fleet_table(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    selected: usize,
    theme: &Theme,
) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("4"),
            title: Some("hosts"),
            sub: Some("by divergence"),
            right: Some("sort ↓ diverge"),
            ..Default::default()
        },
    );

    // The same order the cursor walks — one definition, in `tui::fleet_order_of`.
    let order = super::fleet_order_of(hosts);

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(d::header_cell("host", false, theme)),
        Cell::from(Line::from(d::header_cell("diverge ↓", true, theme)).right_aligned()),
        Cell::from(Line::from(d::header_cell("rtt", false, theme)).right_aligned()),
        Cell::from(d::header_cell("60s", false, theme)),
    ]);

    let at_consensus = hosts.iter().filter(|h| h.diverge_count == Some(0)).count();

    let body: Vec<Row> = order
        .iter()
        .take(inner.height.saturating_sub(2) as usize)
        .map(|&i| {
            let h = &hosts[i];
            // A cursor with no marker is the defect this panel shipped with:
            // arrows moved a selection the screen never showed, and `D` then
            // opened whichever host that invisible index had landed on.
            let selected_row = i == selected;
            let marker = if selected_row { "▌" } else { " " };
            let name_style = if selected_row {
                Style::default()
                    .fg(theme.text_emphasis)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_emphasis)
            };

            let row = match h.diverge_count {
                // Never probed: one honest line, not a row of dashes.
                None => Row::new(vec![
                    Cell::from(Line::from(vec![
                        Span::styled(marker, Style::default().fg(theme.brand)),
                        d::dot(theme.absent),
                    ])),
                    Cell::from(h.name.clone()).style(Style::default().fg(theme.text_secondary)),
                    Cell::from(d::none_line("never probed — no facts to compare", theme)),
                    Cell::from(""),
                    Cell::from(""),
                ]),
                Some(n) => {
                    let col = theme.divergence_count(n);
                    let spark = if h.latency_history.is_empty() {
                        Span::raw("")
                    } else {
                        let ceiling = d::axis_ceiling(&h.latency_history);
                        Span::styled(
                            d::sparkline(&h.latency_history, 12, ceiling),
                            Style::default().fg(theme.muted_good),
                        )
                    };
                    Row::new(vec![
                        Cell::from(Line::from(vec![
                            Span::styled(marker, Style::default().fg(theme.brand)),
                            d::dot(col),
                        ])),
                        Cell::from(h.name.clone()).style(name_style),
                        Cell::from(
                            Line::from(Span::styled(n.to_string(), Style::default().fg(col)))
                                .right_aligned(),
                        ),
                        Cell::from(
                            Line::from(match h.latency_ms {
                                Some(ms) => format!("{:.0}ms", ms),
                                None => "—".into(),
                            })
                            .right_aligned(),
                        )
                        .style(Style::default().fg(theme.text_secondary)),
                        Cell::from(Line::from(spark)),
                    ])
                }
            };

            if selected_row {
                row.style(Style::default().bg(theme.selection_bg))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Percentage(28),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Min(14),
    ];
    f.render_widget(
        Table::new(body, widths).header(header).column_spacing(2),
        inner,
    );

    // Agreement collapses to one quiet line — boring is the correct answer.
    if at_consensus > 0 && inner.height > 2 {
        let row = Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    crate::format::count(at_consensus, "host"),
                    Style::default()
                        .fg(theme.status_good)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " at full consensus",
                    Style::default().fg(theme.text_secondary),
                ),
            ])),
            row,
        );
    }
}

fn render_config_tab(f: &mut Frame, area: Rect, theme: &Theme) {
    let content = Paragraph::new(vec![
        Line::raw(""),
        Line::styled("  Configuration", Style::default().fg(theme.brand).bold()),
        Line::raw(""),
        Line::styled(
            "  Config file: ~/.essh/config.toml",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            "  Cache DB:    ~/.essh/cache.db",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            "  Audit log:   ~/.essh/audit.log",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            format!("  Theme:       {}", theme.name),
            Style::default().fg(theme.text_muted),
        ),
        Line::raw(""),
        Line::styled(
            "  Press 'e' to edit config, or 't' to cycle themes.",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            "  Ctrl+P (F10 in a session) opens the menu — type a theme name to",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            "  jump straight to it.",
            Style::default().fg(theme.text_muted),
        ),
        Line::styled(
            "  Changes reload from ~/.essh/config.toml without restarting.",
            Style::default().fg(theme.text_muted),
        ),
    ])
    .block(
        Block::bordered()
            .title("Config")
            .border_style(Style::default().fg(theme.border)),
    );
    f.render_widget(content, area);
}

fn render_footer(
    f: &mut Frame,
    area: Rect,
    tab: super::DashboardTab,
    status: Option<&str>,
    search_active: bool,
    search_query: &str,
    theme: &Theme,
) {
    let mut lines: Vec<Line> = Vec::new();

    if search_active {
        lines.push(Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme.brand)),
            Span::styled(
                search_query.to_string(),
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("▌", Style::default().fg(theme.brand)),
        ]));
    }

    // The design's footer vocabulary: cyan key, faint label.
    let pairs: &[(&str, &str)] = match tab {
        super::DashboardTab::Fleet => &[
            ("⏎", "connect"),
            ("D", "divergence"),
            ("P", "probe fleet"),
            ("R", "reload"),
            ("^P", "palette"),
            ("q", "quit"),
        ],
        super::DashboardTab::Hosts => &[
            ("⏎", "connect"),
            ("D", "divergence"),
            ("/", "search"),
            ("a", "add"),
            ("p", "probe"),
            ("P", "probe fleet"),
            ("R", "reload"),
            ("^P", "palette"),
            ("q", "quit"),
        ],
        _ => &[("⏎", "connect"), ("^P", "palette"), ("q", "quit")],
    };
    lines.push(d::footer_line(pairs, theme));

    if let Some(msg) = status {
        lines.push(Line::from(Span::styled(
            format!(" {}", msg),
            Style::default().fg(theme.status_warn),
        )));
    }

    f.render_widget(Paragraph::new(lines), area);
}

pub fn render_add_host_dialog(
    f: &mut Frame,
    editing: bool,
    input: &str,
    error: Option<&str>,
    theme: &Theme,
) {
    let area = f.area();
    let popup_width = 66u16.min(area.width.saturating_sub(4));
    let popup_height = 8u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup);

    let surface_style = Style::default()
        .fg(theme.text_primary)
        .bg(theme.selection_bg);
    let title = if editing { " Edit Host " } else { " Add Host " };
    let hint = if editing {
        "  Update user@host[:port] or host[:port] for the selected host."
    } else {
        "  Enter user@host[:port] or host[:port]."
    };

    let block = Block::default()
        .title(title)
        .title_style(
            Style::default()
                .fg(theme.brand)
                .bg(theme.selection_bg)
                .bold(),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.brand).bg(theme.selection_bg))
        .style(surface_style);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut lines = vec![
        Line::styled(
            hint,
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.selection_bg),
        ),
        Line::styled("", surface_style),
        Line::from(vec![
            Span::styled(
                "  > ",
                Style::default()
                    .fg(theme.brand)
                    .bg(theme.selection_bg)
                    .bold(),
            ),
            Span::styled(
                input,
                Style::default()
                    .fg(theme.text_primary)
                    .bg(theme.selection_bg),
            ),
            Span::styled("█", Style::default().fg(theme.brand).bg(theme.selection_bg)),
        ]),
        Line::styled("", surface_style),
    ];

    if let Some(error) = error {
        lines.push(Line::styled(
            format!("  {}", error),
            Style::default()
                .fg(theme.status_error)
                .bg(theme.selection_bg),
        ));
    } else {
        lines.push(Line::styled(
            "  Enter: save  Esc: cancel",
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.selection_bg),
        ));
    }

    let paragraph = Paragraph::new(lines).style(surface_style).block(
        Block::default()
            .style(surface_style)
            .border_style(Style::default().fg(theme.border).bg(theme.selection_bg)),
    );
    f.render_widget(paragraph, inner);
}

#[cfg(test)]
mod fleet_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn facets(n: usize) -> Vec<(String, f64)> {
        (0..n)
            .map(|i| (format!("facet-{i:02}"), if i == 3 { 0.5 } else { 1.0 }))
            .collect()
    }

    fn screen(width: u16, height: u16, n: usize) -> String {
        let theme = crate::theme::dark();
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        let agreement = facets(n);
        term.draw(|f| {
            render_fleet_tab(f, f.area(), &[], &[], None, &agreement, &[], 0, &theme);
        })
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

    /// Every facet must be accounted for — shown, or counted as hidden.
    ///
    /// The box was a fixed six rows, so it displayed four of sixteen checks
    /// and dropped the other twelve without a word. A consensus panel that
    /// quietly omits checks is worse than no panel: it reports agreement it
    /// never looked at.
    #[test]
    fn no_facet_is_dropped_without_saying_so() {
        for (w, h) in [(160u16, 40u16), (100, 30), (80, 24)] {
            let s = screen(w, h, 16);
            let shown = (0..16)
                .filter(|i| s.contains(&format!("facet-{i:02}")))
                .count();
            assert!(
                shown == 16 || s.contains("more facets"),
                "{w}x{h}: showed {shown}/16 facets and never said the rest were hidden:\n{s}"
            );
        }
    }

    /// A wide window should fit all sixteen without a "more" note at all.
    #[test]
    fn a_wide_window_shows_every_facet() {
        let s = screen(160, 40, 16);
        for i in 0..16 {
            assert!(
                s.contains(&format!("facet-{i:02}")),
                "facet-{i:02} missing:\n{s}"
            );
        }
        assert!(!s.contains("more facets"), "nothing should be hidden:\n{s}");
    }

    /// The Fleet screen must carry the verdict, not just a count.
    ///
    /// The handoff leads this screen with reasoning — which host, which
    /// facet, whether it is one cause or two. A percentage and a table say
    /// something is wrong without saying what, which is the thing the
    /// redesign existed to fix.
    #[test]
    fn the_fleet_screen_shows_the_verdict() {
        use crate::divergence::{FacetKey, Verdict};
        let theme = crate::theme::dark();
        let verdicts = vec![
            (
                "10.0.1.10".to_string(),
                Verdict {
                    text: "is a kernel behind and carries a hand-edited nginx.conf".into(),
                    evidence: vec![FacetKey::Kernel],
                    pattern: "test",
                },
            ),
            (
                "10.0.5.30".to_string(),
                Verdict {
                    text: "diverges only on disk".into(),
                    evidence: vec![],
                    pattern: "test",
                },
            ),
        ];
        let agreement = facets(4);
        let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
        term.draw(|f| {
            render_fleet_tab(
                f,
                f.area(),
                &[],
                &[],
                None,
                &agreement,
                &verdicts,
                0,
                &theme,
            );
        })
        .unwrap();
        let buf = term.backend().buffer();
        let screen: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(screen.contains("verdict"), "no verdict panel:\n{screen}");
        assert!(
            screen.contains("10.0.1.10"),
            "the host is not named:\n{screen}"
        );
        assert!(
            screen.contains("kernel behind"),
            "the reasoning is missing:\n{screen}"
        );
        // The evidence keeps the claim checkable.
        assert!(screen.contains("[kernel]"), "no evidence shown:\n{screen}");
    }

    /// With nothing diverging there is nothing to reason about, and the panel
    /// must not appear as an empty box.
    fn groups_screen(groups: &[crate::divergence::GroupSummary]) -> String {
        let theme = crate::theme::dark();
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        term.draw(|f| render_groups_panel(f, f.area(), groups, &theme))
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

    fn group(
        label: &str,
        hosts: usize,
        at: usize,
        unprobed: usize,
    ) -> crate::divergence::GroupSummary {
        crate::divergence::GroupSummary {
            label: label.into(),
            host_count: hosts,
            at_consensus: at,
            unprobed,
            breakers: Vec::new(),
            distinct_values: 1,
            tied_facets: 0,
        }
    }

    /// The `role=web` row from the v0.3.3 dashboard printed "2 / 2 2 / 2 full
    /// consensus": the AT CONSENSUS column drew the ratio, and `note()` drew
    /// it again.
    #[test]
    fn a_consensus_ratio_is_printed_exactly_once() {
        let s = groups_screen(&[group("role=web", 4, 2, 2)]);
        let row = s
            .lines()
            .find(|l| l.contains("role=web"))
            .expect("the group row renders")
            .to_string();

        assert_eq!(
            row.matches("2 / 2").count(),
            1,
            "the ratio is printed twice:\n{row}"
        );
        assert!(row.contains("full consensus"), "{row}");
        assert!(
            row.contains("2 unprobed, excluded"),
            "the exclusion has to be stated, not implied:\n{row}"
        );
    }

    /// Two hosts holding two values is a disagreement, not a winner and a
    /// loser. v0.3.3 printed `1 / 2` and picked the winner by string order.
    #[test]
    fn a_group_with_no_majority_shows_no_ratio() {
        let mut g = group("role=db", 2, 0, 0);
        g.tied_facets = 1;
        g.distinct_values = 2;
        let s = groups_screen(&[g]);
        let row = s
            .lines()
            .find(|l| l.contains("role=db"))
            .expect("the group row renders")
            .to_string();

        assert!(row.contains('—'), "no em dash for the absent ratio:\n{row}");
        assert!(!row.contains("1 / 2"), "a ratio implying a winner:\n{row}");
        assert!(row.contains("no majority"), "{row}");
    }

    #[test]
    fn a_fleet_at_consensus_shows_no_verdict_box() {
        let theme = crate::theme::dark();
        let agreement = facets(4);
        let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
        term.draw(|f| {
            render_fleet_tab(f, f.area(), &[], &[], None, &agreement, &[], 0, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let screen: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !screen.contains("verdict"),
            "empty verdict box drawn:\n{screen}"
        );
    }

    /// SPEC.md §2.1 — agreement needs no graphic.
    ///
    /// Dimming the 100% bars was the earlier mitigation and it did not work:
    /// full-width bars in adjacent rows merge into a solid rectangle whatever
    /// colour they are, and the short bar that is the actual finding gets lost
    /// inside it.
    #[test]
    fn a_facet_at_full_agreement_draws_no_meter() {
        let agreement = vec![
            ("listening ports".to_string(), 0.57),
            ("timezone".to_string(), 0.86),
            ("kernel".to_string(), 1.0),
            ("os release".to_string(), 1.0),
        ];
        let theme = crate::theme::dark();
        let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
        term.draw(|f| {
            render_fleet_tab(f, f.area(), &[], &[], None, &agreement, &[], 0, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();

        // Count meter cells inside each facet's own cell — four facets share a
        // row, so counting the whole line credits one facet's bar to another.
        // A cell runs from the end of its label to its percentage.
        let mut bars_on: std::collections::HashMap<String, usize> = Default::default();
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect();
            for name in ["listening ports", "timezone", "kernel", "os release"] {
                let Some(start) = line.find(name) else {
                    continue;
                };
                let after = &line[start + name.len()..];
                let end = after.find('%').map(|i| i + 1).unwrap_or(after.len());
                *bars_on.entry(name.to_string()).or_default() += after[..end].matches('█').count();
            }
        }

        assert_eq!(
            bars_on.get("kernel").copied().unwrap_or(0),
            0,
            "a facet at 100% drew a meter"
        );
        assert_eq!(
            bars_on.get("os release").copied().unwrap_or(0),
            0,
            "a facet at 100% drew a meter"
        );
        assert!(
            bars_on.get("listening ports").copied().unwrap_or(0) > 0,
            "a diverging facet drew no meter"
        );
    }

    /// The grid must not overrun its box into the panel below.
    #[test]
    fn the_facet_grid_stays_inside_its_box() {
        let s = screen(160, 40, 16);
        let hosts_row = s
            .lines()
            .position(|l| l.contains("hosts"))
            .expect("the hosts panel should render");
        let last_facet = s
            .lines()
            .position(|l| l.contains("facet-15"))
            .expect("the last facet should render");
        assert!(
            last_facet < hosts_row,
            "the facet grid ran past its box into the hosts panel"
        );
    }
}

#[cfg(test)]
mod verdict_tests {
    use super::{display_width, truncate_with_ellipsis};

    #[test]
    fn a_verdict_that_fits_is_left_alone() {
        let s = "os release is behind its peers";
        assert_eq!(truncate_with_ellipsis(s, 40), s);
        assert_eq!(truncate_with_ellipsis(s, s.len()), s);
    }

    #[test]
    fn a_cut_verdict_says_it_was_cut() {
        // The bug: `Paragraph` clipped at the box edge with no mark, turning
        // "ESSH does not infer a shared cause" into "ESSH does not inf" — a
        // sentence that reads as a rendering fault rather than as more text.
        let s = "os release is behind its peers and nginx.conf also differs here";
        let out = truncate_with_ellipsis(s, 30);
        assert!(out.ends_with('…'), "no mark: {out:?}");
        assert!(display_width(&out) <= 30, "overflowed: {out:?}");
        // And it should not cut mid-word when a boundary is close behind.
        assert!(
            !out.trim_end_matches('…').ends_with(char::is_alphabetic)
                || out.trim_end_matches('…').ends_with(' ')
                || !s[..out.chars().count()].ends_with(char::is_alphabetic)
        );
    }

    #[test]
    fn no_room_says_nothing_rather_than_just_an_ellipsis() {
        assert_eq!(truncate_with_ellipsis("anything", 1), "");
        assert_eq!(truncate_with_ellipsis("anything", 0), "");
    }

    #[test]
    fn width_is_display_width_not_byte_count() {
        // A CJK hostname occupies two columns per character; counting bytes
        // would overflow the box and shear every column to its right.
        assert_eq!(display_width("主機"), 4);
        let out = truncate_with_ellipsis("主機主機主機", 5);
        assert!(
            display_width(&out) <= 5,
            "{out:?} is {} wide",
            display_width(&out)
        );
    }
}
