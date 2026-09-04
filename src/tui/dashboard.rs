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
            // One full-width panel. v1 filled the space under a short host
            // list with a groups panel, which was the fleet-consensus screen
            // leaking onto the host list; v2 gives the table the room and
            // lets it breathe.
            render_hosts_tab(
                f,
                chunks[1],
                hosts,
                filtered_indices,
                selected_host,
                table_state,
                theme,
            );
        }
        super::DashboardTab::Health => render_health_tab(f, chunks[1], hosts, selected_host, theme),
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
            out.extend(d::fit_left(
                &d::sparkline(
                    &h.latency_history,
                    self.spark,
                    d::rtt_ceiling(&h.latency_history),
                ),
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
        // Both offline forms lead with the word, so the row says its state
        // before it says its history. "no answer since essh started" is true
        // and still does not tell you which of SPEC §1's states you are
        // looking at.
        _ if h.last_seen.is_empty() => "offline · never answered".to_string(),
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
        let mut out = what.to_string();
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

/// Screen 3 — health. What needs looking at, why, and what is doing it.
///
/// v1's screen 3 was fleet configuration consensus. v2 replaces it: the
/// handoff's whole argument is that consensus percentages ask the reader to
/// interpret a ratio, and this screen answers in sentences instead. SPEC §8
/// says where the consensus screen went and what that costs.
fn render_health_tab(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    selected: usize,
    theme: &Theme,
) {
    let attention: Vec<&super::HostDisplay> = {
        let mut a: Vec<&super::HostDisplay> = hosts
            .iter()
            .filter(|h| h.state().needs_attention())
            .collect();
        // Worst first: unreachable before busy, and within each, the order
        // the host list is in, so the screen does not reshuffle every sweep.
        a.sort_by_key(|h| match h.state() {
            super::HostState::Dropped => 0,
            super::HostState::Offline => 1,
            _ => 2,
        });
        a
    };

    // Every panel is sized to its content. A box stretched over empty space
    // reads as missing data rather than as an absence of problems.
    let attention_height = (attention.len().max(1) as u16 + 3).min(area.height / 2);
    let card_height = if attention.is_empty() { 0 } else { 9 };
    // The bottom row is as tall as its taller panel's content: reachability
    // is one row per host, busiest is five. Anything beyond that is empty
    // box, which reads as missing data rather than as nothing to report.
    let bottom_height = (hosts.len().max(5) as u16 + 2)
        .min(area.height.saturating_sub(attention_height + card_height));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(attention_height),
            Constraint::Length(card_height),
            Constraint::Length(bottom_height),
            Constraint::Min(0),
        ])
        .split(area);

    render_attention(f, rows[0], hosts, &attention, theme);
    if card_height > 0 {
        render_health_cards(f, rows[1], &attention, theme);
    }
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[2]);
    render_busiest(f, bottom[0], hosts, theme);
    render_reachability(f, bottom[1], hosts, selected, theme);
}

/// Panel 1 — the most important panel in v2.
///
/// It says in words which hosts to look at, and it says so when nothing needs
/// looking at, which is the state a dashboard spends most of its life in. Each
/// row names the cause when one is known: a busy host with a named cause is
/// not an incident, and a row that reports 94% without saying what is using it
/// hands the reader the same work the consensus screen did.
fn render_attention(
    f: &mut Frame,
    area: Rect,
    hosts: &[super::HostDisplay],
    attention: &[&super::HostDisplay],
    theme: &Theme,
) {
    let sub = format!(
        "{} of {}",
        attention.len(),
        crate::format::count(hosts.len(), "host")
    );
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("needs attention"),
            sub: Some(&sub),
            foot_left: &[("⏎", " open host"), ("r", " reconnect")],
            ..Default::default()
        },
    );
    if inner.width < 30 || inner.height == 0 {
        return;
    }

    if attention.is_empty() {
        let reachable = hosts.iter().filter(|h| h.status.is_reachable()).count();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "nothing needs attention",
                    Style::default().fg(theme.status_good),
                ),
                Span::styled(
                    format!(
                        " · {} reachable, none over threshold",
                        crate::format::count(reachable, "host")
                    ),
                    Style::default().fg(theme.text_muted),
                ),
            ])),
            inner,
        );
        return;
    }

    // SPEC §3.3: host 15 for `staging-web-01`, verdict 9 for `disk 81%`.
    const NAME: usize = 15;
    const VERDICT: usize = 9;
    let sentence = (inner.width as usize).saturating_sub(NAME + VERDICT + 2 * GAP);

    let lines: Vec<Line> = attention
        .iter()
        .take(inner.height as usize)
        .map(|h| {
            let state = h.state();
            let (verdict, why) = attention_wording(h);
            let mut spans = d::fit_left(
                &h.name,
                NAME,
                Style::default().fg(state.color(theme)).bold(),
            );
            spans.push(Span::raw(" ".repeat(GAP)));
            spans.extend(d::fit_left(
                &verdict,
                VERDICT,
                Style::default().fg(state.color(theme)),
            ));
            spans.push(Span::raw(" ".repeat(GAP)));
            spans.extend(d::fit_left(
                &why,
                sentence,
                Style::default().fg(theme.text_secondary),
            ));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// The verdict and the sentence under it.
///
/// The sentence carries the cause and, where there is one, the reassurance.
/// "cpu 94%" alone is a number to interpret; "over 90%, cargo build in
/// session 4, expected during a build" is an answer.
fn attention_wording(h: &super::HostDisplay) -> (String, String) {
    let state = h.state();
    match state {
        super::HostState::Dropped | super::HostState::Offline => {
            let sessions = h.session_count;
            let tail = if sessions > 0 {
                format!(
                    "{} waiting to reconnect · scrollback preserved",
                    crate::format::count(sessions, "session")
                )
            } else {
                "no session was open".to_string()
            };
            let head = if state == super::HostState::Dropped {
                format!(
                    "connection reset {}",
                    crate::format::relative_time(&h.last_seen)
                )
            } else {
                format!("no answer on {}:{}", h.hostname, h.port)
            };
            (state.word().to_string(), format!("{head} · {tail}"))
        }
        _ => {
            let Some((kind, pct)) = h.vitals.crossed() else {
                return ("busy".into(), String::new());
            };
            let verdict = format!("{kind} {}%", pct.round());
            // The heaviest process is the cause, when we have one. This is
            // the difference between a threshold alert and an explanation.
            let cause = h
                .vitals
                .processes
                .first()
                .map(|p| format!(" — {} at {:.0}%", p.name, p.cpu_pct))
                .unwrap_or_default();
            let sustained = h
                .vitals
                .cpu_over_since
                .filter(|_| kind == "cpu")
                .map(|since| {
                    format!(
                        "over {}% for {}m",
                        tui_cpu_threshold(),
                        since.elapsed().as_secs() / 60
                    )
                })
                .unwrap_or_else(|| format!("{kind} at {}%", pct.round()));
            (verdict, format!("{sustained}{cause}"))
        }
    }
}

fn tui_cpu_threshold() -> i64 {
    super::CPU_BUSY_PCT as i64
}

/// The cards. One per attention host, four across.
///
/// The two stale cards are not the same stale, and SPEC §2.4 keeps them
/// apart: a dropped host has a last reading and says so, and a host that
/// never answered has nothing and says that instead. Rendering the second as
/// the first would put invented numbers on screen with a label claiming they
/// were measured.
fn render_health_cards(
    f: &mut Frame,
    area: Rect,
    attention: &[&super::HostDisplay],
    theme: &Theme,
) {
    let n = attention.len().min(4);
    if n == 0 || area.height < 4 {
        return;
    }
    // Cards keep their width whether there is one of them or four. A single
    // card stretched across 146 cells puts its label at one edge of the
    // screen and its value at the other, with a hundred cells of nothing
    // between them, and reads as a broken row rather than as one card.
    const CARD: u16 = 38;
    let mut widths: Vec<Constraint> = vec![Constraint::Length(CARD); n];
    widths.push(Constraint::Min(0));
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(widths)
        .split(area);

    for (slot, h) in cols.iter().zip(attention.iter().take(n)) {
        let state = h.state();
        let sessions = if h.session_count == 0 {
            state.word().to_string()
        } else {
            crate::format::count(h.session_count, "session")
        };
        let inner = d::pane(
            f,
            *slot,
            theme,
            &d::PanelOpts {
                title: Some(&h.name),
                sub: Some(&sessions),
                ..Default::default()
            },
        );
        if inner.width < 12 || inner.height < 3 {
            continue;
        }
        let stale = state.recovery().is_some();
        let w = inner.width as usize;
        let mut lines: Vec<Line> = Vec::new();

        // The headline figure. `—` when there is nothing live to show, at
        // the same size, so the card's shape does not change when a host
        // goes away.
        let figure = h
            .vitals
            .cpu_pct
            .filter(|_| !stale)
            .map(|c| format!("{}%", c.round()))
            .unwrap_or_else(|| "—".into());
        let load = h
            .vitals
            .load1
            .zip(h.vitals.cores)
            .filter(|_| !stale)
            .map(|(l, c)| format!("load {l:.2} · {c} cores"))
            .unwrap_or_default();
        let mut head = vec![Span::styled(
            figure,
            Style::default()
                .fg(if stale {
                    theme.absent
                } else {
                    theme.text_emphasis
                })
                .bold(),
        )];
        head.push(Span::styled(" cpu", Style::default().fg(theme.text_muted)));
        let used: usize = head.iter().map(|s| d::width_of(&s.content)).sum();
        head.extend(d::fit_right(
            &load,
            w.saturating_sub(used),
            Style::default().fg(theme.text_muted),
        ));
        lines.push(Line::from(head));

        for (label, pct, colour) in [
            ("mem", h.vitals.mem_pct, theme.status_info),
            (
                "disk",
                h.vitals.disk_pct,
                if h.vitals.disk_pct.is_some_and(|d| d >= 80.0) {
                    theme.status_warn
                } else {
                    theme.muted_good
                },
            ),
        ] {
            let mut row = d::fit_left(label, 5, Style::default().fg(theme.text_muted));
            match pct.filter(|_| !stale) {
                Some(p) => {
                    row.extend(d::meter_flat(
                        p / 100.0,
                        w.saturating_sub(11),
                        colour,
                        theme,
                    ));
                    row.extend(d::fit_right(
                        &format!("{}%", p.round()),
                        5,
                        Style::default().fg(theme.text_secondary),
                    ));
                }
                None => row.extend(d::fit_right(
                    "—",
                    w.saturating_sub(5),
                    Style::default().fg(theme.absent),
                )),
            }
            lines.push(Line::from(row));
        }

        // SPEC §2.4. Never render stale numbers as if they were live, and
        // never claim a last reading for a host that never gave one.
        let note = if stale {
            match (state, h.vitals.cpu_pct) {
                (super::HostState::Dropped, Some(cpu)) => {
                    format!("Connection reset. Last reading {}%, not live.", cpu.round())
                }
                (super::HostState::Dropped, None) => {
                    "Connection reset. No reading was taken.".to_string()
                }
                _ => "Never reached. Nothing to show.".to_string(),
            }
        } else {
            h.vitals
                .processes
                .first()
                .map(|p| format!("{} at {:.0}%", p.name, p.cpu_pct))
                .unwrap_or_else(|| "no process stands out".to_string())
        };
        lines.push(Line::from(Span::styled(
            d::ellipsise(&note, w),
            Style::default().fg(if stale {
                theme.absent
            } else {
                theme.text_muted
            }),
        )));
        if let Some(key) = state.recovery() {
            lines.push(Line::from(Span::styled(
                key.to_string(),
                Style::default().fg(theme.key_hint),
            )));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
}

/// One row of the busiest list: a process, or a run of identical siblings.
#[derive(Debug, PartialEq)]
pub struct Busiest {
    pub host: String,
    pub name: String,
    pub cpu_pct: f64,
    pub rss_kb: u64,
    /// How many identical siblings this row stands for.
    pub siblings: usize,
}

impl Busiest {
    /// The process, and how many of it, inside `width` cells.
    ///
    /// The `×N` is reserved before the name is cut, not appended after. Doing
    /// it the other way round truncates the count off the end of a long
    /// command, which turns a collapsed row back into an ordinary one that
    /// happens to be reporting 296% CPU.
    fn label(&self, width: usize) -> String {
        if self.siblings <= 1 {
            return d::ellipsise(&self.name, width);
        }
        let tail = format!(" ×{}", self.siblings);
        let room = width.saturating_sub(d::width_of(&tail));
        format!("{}{tail}", d::ellipsise(&self.name, room))
    }
}

/// Every reachable host's processes, siblings collapsed, hottest first.
///
/// Defect 7 of the v0.3.3 list: eleven of fifteen process rows were identical
/// nginx workers, so the panel spent its height telling you the same thing
/// eleven times. Collapsing them is not cosmetic — it is the difference
/// between a list of the busiest processes and a list of one process.
///
/// Unreachable hosts are excluded. Their last process list is a snapshot of
/// what *was* running, and "busiest right now" is a claim about now.
pub fn busiest(hosts: &[super::HostDisplay]) -> Vec<Busiest> {
    let mut rows: Vec<Busiest> = Vec::new();
    for h in hosts.iter().filter(|h| h.state().recovery().is_none()) {
        for p in &h.vitals.processes {
            match rows
                .iter_mut()
                .find(|r| r.host == h.name && r.name == p.name)
            {
                Some(r) => {
                    r.cpu_pct += p.cpu_pct;
                    r.rss_kb += p.mem_rss_kb;
                    r.siblings += 1;
                }
                None => rows.push(Busiest {
                    host: h.name.clone(),
                    name: p.name.clone(),
                    cpu_pct: p.cpu_pct,
                    rss_kb: p.mem_rss_kb,
                    siblings: 1,
                }),
            }
        }
    }
    rows.sort_by(|a, b| {
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Ties by name, so a fleet at rest does not reshuffle every sweep.
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// Panel 2 — what is eating the fleet, without opening a host.
///
/// One list across every host that answered. A top-N list does not sum to any
/// host's headline and must not look as though it should: the tail is real.
fn render_busiest(f: &mut Frame, area: Rect, hosts: &[super::HostDisplay], theme: &Theme) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("2"),
            title: Some("busiest right now"),
            sub: Some("across all hosts"),
            ..Default::default()
        },
    );
    if inner.width < 30 || inner.height < 2 {
        return;
    }

    let all = busiest(hosts);

    // SPEC §3.3: HOST 14, CPU 5, RSS 7. `16.8 MB` is seven characters, which
    // is what the handoff's six would have clipped.
    const HOST: usize = 14;
    const CPU: usize = 5;
    const RSS: usize = 7;
    let proc_w = (inner.width as usize).saturating_sub(HOST + CPU + RSS + 3 * GAP);

    let mut lines = vec![Line::from({
        let mut h = d::fit_left("HOST", HOST, Style::default().fg(theme.text_muted));
        h.push(Span::raw(" ".repeat(GAP)));
        h.extend(d::fit_left(
            "PROCESS",
            proc_w,
            Style::default().fg(theme.text_muted),
        ));
        h.push(Span::raw(" ".repeat(GAP)));
        h.extend(d::fit_right(
            "CPU",
            CPU,
            Style::default().fg(theme.text_muted),
        ));
        h.push(Span::raw(" ".repeat(GAP)));
        h.extend(d::fit_right(
            "RSS",
            RSS,
            Style::default().fg(theme.text_muted),
        ));
        h
    })];

    if all.is_empty() {
        lines.push(d::none_line("no process readings yet", theme));
    }
    for p in all.iter().take(inner.height.saturating_sub(1) as usize) {
        let mut row = d::fit_left(&p.host, HOST, Style::default().fg(theme.text_secondary));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_left(
            &p.label(proc_w),
            proc_w,
            Style::default().fg(theme.text_primary),
        ));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_right(
            &format!("{:.1}", p.cpu_pct),
            CPU,
            Style::default().fg(theme.bounded_bad(p.cpu_pct)),
        ));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_right(
            &widgets::format_bytes(p.rss_kb * 1024),
            RSS,
            Style::default().fg(theme.text_muted),
        ));
        lines.push(Line::from(row));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Panel 3 — reachability, one row per host.
///
/// The sparkline takes the Fill width rather than the handoff's twenty cells:
/// twenty is the reference's pixel width, and at 150 columns there are
/// forty-odd cells here for free.
fn render_reachability(
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
            key: Some("3"),
            title: Some("reachability"),
            sub: Some("rtt"),
            ..Default::default()
        },
    );
    if inner.width < 30 || inner.height == 0 {
        return;
    }
    const NAME: usize = 15;
    const RTT: usize = 6;
    let spark = (inner.width as usize).saturating_sub(NAME + RTT + 2 * GAP);

    let lines: Vec<Line> = hosts
        .iter()
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, h)| {
            let state = h.state();
            let mut row = d::fit_left(
                &h.name,
                NAME,
                Style::default().fg(if i == selected {
                    theme.text_emphasis
                } else {
                    theme.text_secondary
                }),
            );
            row.push(Span::raw(" ".repeat(GAP)));
            row.extend(d::fit_left(
                &d::sparkline(
                    &h.latency_history,
                    spark,
                    d::rtt_ceiling(&h.latency_history),
                ),
                spark,
                Style::default().fg(match state {
                    super::HostState::Up => theme.muted_good,
                    super::HostState::Busy => theme.status_warn,
                    super::HostState::Dropped => theme.status_error,
                    _ => theme.absent,
                }),
            ));
            row.push(Span::raw(" ".repeat(GAP)));
            // `×` for a host that is not answering, never a zero. A `0.0` in
            // an RTT column is the fastest host on the screen.
            row.extend(d::fit_right(
                &h.latency_ms
                    .map(|ms| format!("{ms:.1}"))
                    .unwrap_or_else(|| "×".into()),
                RTT,
                Style::default().fg(if h.latency_ms.is_some() {
                    theme.text_secondary
                } else {
                    state.color(theme)
                }),
            ));
            Line::from(row)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
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
    // SPEC §4.2 — per screen, then a tail that is bound everywhere. The
    // handoff prints one footer on all six screens, which is how it comes to
    // advertise `d detach` on the screen where `d` is download.
    let pairs: &[(&str, &str)] = match tab {
        super::DashboardTab::Health => &[
            ("⏎", "open host"),
            ("r", "reconnect"),
            ("^P", "menu"),
            ("q", "quit"),
        ],
        super::DashboardTab::Hosts => &[
            ("⏎", "connect"),
            ("r", "reconnect"),
            ("m", "monitor"),
            ("a", "add"),
            ("e", "edit"),
            ("/", "search"),
            ("D", "divergence"),
            ("^P", "menu"),
            ("q", "quit"),
        ],
        _ => &[("⏎", "connect"), ("^P", "menu"), ("q", "quit")],
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
