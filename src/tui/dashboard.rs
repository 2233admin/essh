use crate::design as d;
use crate::session::Session;
use crate::theme::Theme;
use crate::tui::widgets;
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph, TableState},
};

/// Render the dashboard view (no active session focused)
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    sessions: &[Session],
    session_rows: &[SessionRow],
    quick: &[QuickHost],
    activity: &[super::activity::Event],
    broadcast: &super::broadcast::Broadcast,
    sessions_panel: super::SessionsPanel,
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
            Constraint::Length(1),             // the reference's band under the tabs
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
        super::DashboardTab::Sessions => render_sessions_tab(
            f,
            chunks[2],
            session_rows,
            quick,
            activity,
            broadcast,
            sessions_panel,
            selected_session,
            theme,
        ),
        super::DashboardTab::Hosts => {
            // One full-width panel. v1 filled the space under a short host
            // list with a groups panel, which was the fleet-consensus screen
            // leaking onto the host list; v2 gives the table the room and
            // lets it breathe.
            render_hosts_tab(
                f,
                chunks[2],
                hosts,
                filtered_indices,
                selected_host,
                table_state,
                theme,
            );
        }
        super::DashboardTab::Health => render_health_tab(f, chunks[2], hosts, selected_host, theme),
        super::DashboardTab::Config => render_config_tab(f, chunks[2], theme),
    }

    render_footer(
        f,
        chunks[3],
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

/// Screen 1 — sessions. The screen v2 opens on.
///
/// Sessions are the primary object and hosts are how you get more of them,
/// which is the whole rearrangement: v1 opened on a host list and made you
/// find your way to what you were already running.
#[allow(clippy::too_many_arguments)]
fn render_sessions_tab(
    f: &mut Frame,
    area: Rect,
    app_sessions: &[SessionRow],
    quick: &[QuickHost],
    activity: &[super::activity::Event],
    broadcast: &super::broadcast::Broadcast,
    focus: super::SessionsPanel,
    selected: usize,
    theme: &Theme,
) {
    // The table takes what it needs and the row of three takes the rest, up
    // to what its own content wants. A panel stretched over empty space reads
    // as missing data.
    let table_height = ((app_sessions.len().max(1) as u16) + 3).min(area.height / 2);
    let row_height = quick
        .len()
        .max(activity.len())
        .max(broadcast.replies.len() + 3)
        .max(4) as u16
        + 4;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(table_height),
            Constraint::Length(1),
            Constraint::Length(row_height.min(area.height.saturating_sub(table_height + 1))),
            Constraint::Min(0),
        ])
        .split(area);

    render_session_table(f, rows[0], app_sessions, focus, selected, theme);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Length(2),
            Constraint::Ratio(1, 3),
            Constraint::Length(2),
            Constraint::Min(20),
        ])
        .split(rows[2]);
    render_quick_connect(f, cols[0], quick, focus, theme);
    render_broadcast(f, cols[2], broadcast, focus, theme);
    render_activity(f, cols[4], activity, theme);
}

/// One row of the sessions table, flattened out of the app so the renderer
/// takes data rather than five collections and an index.
pub struct SessionRow {
    pub number: usize,
    pub host: String,
    pub state: &'static str,
    pub host_state: super::HostState,
    pub running: Option<String>,
    pub up: String,
    pub cpu_pct: Option<f64>,
    pub last_line: Option<String>,
    pub idle: String,
    pub dropped: bool,
    /// What to show where a value would go, when there is none. SPEC §6.2:
    /// three different absences, three different words.
    pub absent: &'static str,
}

/// A host with no session, for the quick-connect panel.
pub struct QuickHost {
    pub name: String,
    pub state: super::HostState,
}

const SESSION_GAP: usize = 2;

fn render_session_table(
    f: &mut Frame,
    area: Rect,
    sessions: &[SessionRow],
    focus: super::SessionsPanel,
    selected: usize,
    theme: &Theme,
) {
    let hosts = sessions
        .iter()
        .map(|s| s.host.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let sub = format!(
        "{} open on {} · Ctrl+A 1-9 jumps",
        crate::format::count(sessions.len(), "session"),
        crate::format::count(hosts, "host")
    );
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("sessions"),
            sub: Some(&sub),
            right: Some("detached survive restarts"),
            focused: focus == super::SessionsPanel::Table,
            foot_left: &[
                ("↑↓", " select"),
                ("⏎", " attach"),
                ("d", " detach"),
                ("n", " new"),
                ("w", " close"),
                ("⇥", " next panel"),
            ],
            ..Default::default()
        },
    );
    if inner.width < 40 || inner.height == 0 {
        return;
    }

    if sessions.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "no sessions open",
                    Style::default().fg(theme.text_secondary),
                ),
                Span::styled(" · press ", Style::default().fg(theme.text_muted)),
                Span::styled("2", Style::default().fg(theme.key_hint)),
                Span::styled(
                    " for hosts, or ⇥ then a number to open one",
                    Style::default().fg(theme.text_muted),
                ),
            ])),
            inner,
        );
        return;
    }

    let cols = SessionCols::fit(inner.width as usize);
    let mut lines = vec![Line::from(cols.header(theme))];
    for (i, s) in sessions.iter().take(inner.height as usize - 1).enumerate() {
        let on = i == selected && focus == super::SessionsPanel::Table;
        lines.push(Line::from(cols.row(s, on, theme)).style(if on {
            Style::default().bg(theme.selection_bg)
        } else {
            Style::default()
        }));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// SPEC §3.1's budget. HOST is 15, not the handoff's 13: `staging-web-01` is
/// fourteen characters and the hosts table already budgets 15 for the same
/// value.
struct SessionCols {
    num: usize,
    host: usize,
    state: usize,
    running: usize,
    up: usize,
    cpu: usize,
    last: usize,
    idle: usize,
}

impl SessionCols {
    fn fit(width: usize) -> Self {
        let mut c = SessionCols {
            num: 3,
            host: 15,
            state: 9,
            running: 11,
            up: 8,
            cpu: 7,
            last: 0,
            idle: 9,
        };
        // LAST LINE is the column the screen exists for, so it takes what is
        // left and everything else gives way to it in the order SPEC §3.6
        // sets: UP, then CPU, then IDLE.
        let fixed =
            |c: &SessionCols| c.num + 1 + c.host + c.state + c.running + c.up + c.cpu + c.idle;
        let gaps = |c: &SessionCols| {
            SESSION_GAP
                * (2 + [c.host, c.state, c.running, c.up, c.cpu, c.idle]
                    .iter()
                    .filter(|w| **w > 0)
                    .count())
        };
        for drop in 0..4 {
            match drop {
                1 => c.up = 0,
                2 => c.cpu = 0,
                3 => c.idle = 0,
                _ => {}
            }
            if width > fixed(&c) + gaps(&c) + 12 {
                c.last = width - fixed(&c) - gaps(&c);
                return c;
            }
        }
        c.last = width.saturating_sub(fixed(&c) + gaps(&c));
        c
    }

    fn header(&self, t: &Theme) -> Vec<Span<'static>> {
        let mut out: Vec<Span> = Vec::new();
        let muted = Style::default().fg(t.text_muted);
        out.extend(d::fit_right("#", self.num, muted));
        // The gap, the dot column the selection marker replaces, and the gap
        // after it. Counting the second gap is the difference between a
        // header that sits over its column and one that sits two cells left
        // of it.
        out.push(Span::raw(" ".repeat(SESSION_GAP + 1 + SESSION_GAP)));
        let mut push = |label: &str, w: usize, right: bool| {
            if w == 0 {
                return;
            }
            if right {
                out.extend(d::fit_right(label, w, muted));
            } else {
                out.extend(d::fit_left(label, w, muted));
            }
            out.push(Span::raw(" ".repeat(SESSION_GAP)));
        };
        push("HOST", self.host, false);
        push("STATE", self.state, false);
        push("RUNNING", self.running, false);
        push("UP", self.up, true);
        push("CPU", self.cpu, true);
        // LAST LINE trails a gap only when IDLE follows it. Trailing one
        // unconditionally spends two cells the budget did not allow for, at
        // exactly the widths where IDLE has already been dropped for lack of
        // room.
        if self.last > 0 {
            out.extend(d::fit_left("LAST LINE", self.last, muted));
            if self.idle > 0 {
                out.push(Span::raw(" ".repeat(SESSION_GAP)));
            }
        }
        if self.idle > 0 {
            out.extend(d::fit_right("IDLE", self.idle, muted));
        }
        out
    }

    fn row(&self, s: &SessionRow, selected: bool, t: &Theme) -> Vec<Span<'static>> {
        let mut out = d::fit_right(
            &s.number.to_string(),
            self.num,
            Style::default()
                .fg(if selected { t.brand } else { t.text_muted })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        );
        out.push(Span::raw(" ".repeat(SESSION_GAP)));
        // The host's state, not the session's: trouble on the machine is
        // visible from the session list without leaving it.
        out.push(if selected {
            Span::styled("▌", Style::default().fg(t.brand))
        } else {
            Span::styled(
                s.host_state.glyph(),
                Style::default().fg(s.host_state.color(t)),
            )
        });
        out.push(Span::raw(" ".repeat(SESSION_GAP)));

        let mut push = |spans: Vec<Span<'static>>, w: usize| {
            if w == 0 {
                return;
            }
            out.extend(spans);
            out.push(Span::raw(" ".repeat(SESSION_GAP)));
        };
        push(
            d::fit_left(
                &s.host,
                self.host,
                Style::default().fg(if selected {
                    t.text_emphasis
                } else {
                    t.text_primary
                }),
            ),
            self.host,
        );
        // `detached` is normal and unalarming: it is what the tool is for.
        let state_colour = match s.state {
            "attached" => t.status_good,
            "dropped" => t.status_error,
            "opening" => t.status_warn,
            _ => t.text_muted,
        };
        push(
            d::fit_left(s.state, self.state, Style::default().fg(state_colour)),
            self.state,
        );
        push(
            d::fit_left(
                s.running.as_deref().unwrap_or(s.absent),
                self.running,
                Style::default().fg(if s.running.is_some() {
                    t.text_primary
                } else {
                    t.absent
                }),
            ),
            self.running,
        );
        push(
            d::fit_right(&s.up, self.up, Style::default().fg(t.text_secondary)),
            self.up,
        );
        push(
            d::fit_right(
                &s.cpu_pct
                    .map(|c| format!("{c:.1}%"))
                    .unwrap_or_else(|| s.absent.to_string()),
                self.cpu,
                Style::default().fg(match s.cpu_pct {
                    // A dropped session has no live tree, so this is `—`, and
                    // a stale percentage in a live column is the defect the
                    // whole state model exists to stop.
                    None => t.absent,
                    Some(c) => t.bounded_bad(c),
                }),
            ),
            self.cpu,
        );

        // The column the screen is for: what a detached session is doing,
        // without attaching to it.
        let (last, last_style) = match (&s.last_line, s.dropped) {
            (_, true) => (
                "connection reset · r reconnect, scrollback kept".to_string(),
                Style::default().fg(t.status_error),
            ),
            (Some(l), _) => (l.clone(), Style::default().fg(t.text_secondary)),
            (None, _) => ("—".into(), Style::default().fg(t.absent)),
        };
        if self.last > 0 {
            out.extend(d::fit_left(&last, self.last, last_style));
            if self.idle > 0 {
                out.push(Span::raw(" ".repeat(SESSION_GAP)));
            }
        }
        if self.idle > 0 {
            out.extend(d::fit_right(
                &s.idle,
                self.idle,
                Style::default().fg(t.text_muted),
            ));
        }
        out
    }
}

/// Panel 2 — the hosts you have no session on, numbered.
///
/// Hosts that already have one are absent by design: they are in the table
/// above. The handoff's own list omits a host that has none, which is what
/// happens when the two are written down separately instead of derived.
fn render_quick_connect(
    f: &mut Frame,
    area: Rect,
    quick: &[QuickHost],
    focus: super::SessionsPanel,
    theme: &Theme,
) {
    let focused = focus == super::SessionsPanel::QuickConnect;
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("2"),
            title: Some("quick connect"),
            sub: Some(&crate::format::count(quick.len(), "host")),
            focused,
            foot_left: if focused {
                &[("1-9", " open"), ("⇥", " next"), ("⎋", " back")]
            } else {
                &[("⇥", " focus")]
            },
            ..Default::default()
        },
    );
    if inner.width < 12 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    if quick.is_empty() {
        lines.push(d::none_line("every host has a session", theme));
    }
    let room = inner.height as usize;
    for (i, h) in quick.iter().take(room.saturating_sub(1)).enumerate() {
        let reachable = h.state.recovery().is_none();
        let mut row = d::fit_right(
            &(i + 1).to_string(),
            2,
            // The number is only a key while the panel holds focus, so it is
            // only lit then. A cyan digit you cannot press is a lie about the
            // keyboard.
            Style::default().fg(if focused && reachable {
                theme.key_hint
            } else {
                theme.text_muted
            }),
        );
        row.push(Span::raw("  "));
        let name_w = (inner.width as usize).saturating_sub(2 + 2 + 11);
        row.extend(d::fit_left(
            &h.name,
            name_w,
            Style::default().fg(if reachable {
                theme.text_primary
            } else {
                theme.text_muted
            }),
        ));
        row.extend(d::fit_right(
            if reachable {
                "no session"
            } else {
                h.state.word()
            },
            11,
            Style::default().fg(if reachable {
                theme.text_muted
            } else {
                h.state.color(theme)
            }),
        ));
        lines.push(Line::from(row));
    }
    if !quick.is_empty() {
        lines.push(Line::from(Span::styled(
            if focused {
                "Press the number to open a session."
            } else {
                "Tab here, then press the number."
            },
            Style::default().fg(theme.text_muted),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Panel 3 — one command, many hosts.
fn render_broadcast(
    f: &mut Frame,
    area: Rect,
    b: &super::broadcast::Broadcast,
    focus: super::SessionsPanel,
    theme: &Theme,
) {
    let focused = focus == super::SessionsPanel::Broadcast;
    let status = b.status();
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("3"),
            title: Some("broadcast"),
            sub: Some(&status),
            focused,
            foot_left: if focused {
                &[
                    ("t", " target"),
                    ("i", " command"),
                    ("⏎", " send"),
                    ("⎋", " back"),
                ]
            } else {
                &[("⇥", " focus")]
            },
            ..Default::default()
        },
    );
    if inner.width < 16 || inner.height < 2 {
        return;
    }

    let caret = |on: bool| {
        if on {
            Span::styled("▌", Style::default().fg(theme.brand))
        } else {
            Span::raw("")
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("to  ", Style::default().fg(theme.text_muted)),
            Span::styled(
                if b.target.is_empty() {
                    "no target".to_string()
                } else {
                    b.target.clone()
                },
                Style::default().fg(if b.target.is_empty() {
                    theme.absent
                } else {
                    theme.text_primary
                }),
            ),
            caret(focused && b.editing && b.editing_target),
        ]),
        Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.brand)),
            Span::styled(
                d::ellipsise(&b.command, (inner.width as usize).saturating_sub(3)),
                Style::default().fg(theme.text_primary),
            ),
            caret(focused && b.editing && !b.editing_target),
        ]),
    ];

    let room = (inner.height as usize).saturating_sub(2);
    for (host, reply) in b.replies.iter().take(room) {
        let colour = match reply {
            r if r.is_running() => theme.status_warn,
            r if r.ok() => theme.status_good,
            _ => theme.status_error,
        };
        let mut row = d::fit_left(host, 11, Style::default().fg(colour));
        row.push(Span::raw(" "));
        row.extend(d::fit_left(
            &reply.summary(),
            (inner.width as usize).saturating_sub(12),
            Style::default().fg(theme.text_muted),
        ));
        lines.push(Line::from(row));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Panel 4 — what changed, newest first.
fn render_activity(f: &mut Frame, area: Rect, events: &[super::activity::Event], theme: &Theme) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("4"),
            title: Some("activity"),
            sub: Some("newest first"),
            ..Default::default()
        },
    );
    if inner.width < 16 || inner.height == 0 {
        return;
    }
    if events.is_empty() {
        f.render_widget(
            Paragraph::new(d::none_line("nothing has changed yet", theme)),
            inner,
        );
        return;
    }

    const AT: usize = 5;
    const HOST: usize = 15;
    let text_w = (inner.width as usize).saturating_sub(AT + HOST + 2 * SESSION_GAP);
    let lines: Vec<Line> = events
        .iter()
        .take(inner.height as usize)
        .map(|e| {
            let mut row = d::fit_left(
                &e.at.format("%H:%M").to_string(),
                AT,
                Style::default().fg(theme.text_muted),
            );
            row.push(Span::raw(" ".repeat(SESSION_GAP)));
            row.extend(d::fit_left(
                &e.host,
                HOST,
                Style::default().fg(e.severity.color(theme)),
            ));
            row.push(Span::raw(" ".repeat(SESSION_GAP)));
            row.extend(d::fit_left(
                &e.text,
                text_w,
                Style::default().fg(theme.text_secondary),
            ));
            Line::from(row)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
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
    full: Rect,
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
    // Sized to its rows. A box stretched to the bottom of the window reads as
    // a list that failed to load rather than as a fleet of eleven.
    let area = Rect {
        height: ((filtered_indices.len() as u16) + 3).min(full.height),
        ..full
    };
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("hosts"),
            sub: Some(&sub),
            // Panel-specific keys only. The screen's footer carries the
            // ones bound everywhere, and printing both lists twice is how a
            // screen ends up with two footers disagreeing about which keys
            // it has.
            foot_left: &[
                ("↑↓", " select"),
                ("⏎", " connect"),
                ("r", " reconnect"),
                ("D", " divergence"),
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
        // One gap before every column after the state glyph, TAGS included.
        // Counting seven of them and forgetting the eighth is how the header
        // came out four cells wider than the panel it sits in, which pushes
        // the last column off the right edge and looks like a table that
        // "just does not fit" rather than like arithmetic.
        let gaps = |c: &HostCols| {
            GAP * (2 + [c.address, c.sess, c.cpu, c.mem, c.disk, c.rtt, c.spark]
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
        let mut out = vec![Span::raw(" ")];
        // The gap goes *before* each column, matching `row`. Trailing it
        // instead adds one the budget did not allow for, at the one edge
        // where nothing follows to make it visible.
        let mut push = |label: &str, w: usize, right: bool| {
            if w == 0 {
                return;
            }
            out.push(Span::raw(" ".repeat(GAP)));
            let cell = if right {
                d::fit_right(label, w, Style::default().fg(t.text_muted))
            } else {
                d::fit_left(label, w, Style::default().fg(t.text_muted))
            };
            out.extend(cell);
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
    // Five content lines plus the border. `pane` hangs its title on the top
    // edge and its hints on the bottom, so a panel taller than its content
    // shows the gap between the two, which reads as a row that failed to
    // render.
    let card_height = if attention.is_empty() { 0 } else { 7 };
    // The bottom row is as tall as its taller panel's content: reachability
    // is one row per host, busiest is five. Anything beyond that is empty
    // box, which reads as missing data rather than as nothing to report.
    let bottom_height = (hosts.len().max(5) as u16 + 3).min(
        area.height
            .saturating_sub(attention_height + card_height + 2),
    );
    // The reference's 20-26px panel gaps. Without them a panel's bottom hints
    // sit on the row above the next panel's title, and the two read as one
    // broken border.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(attention_height),
            Constraint::Length(1),
            Constraint::Length(card_height),
            Constraint::Length(if card_height > 0 { 1 } else { 0 }),
            Constraint::Length(bottom_height),
            Constraint::Min(0),
        ])
        .split(area);

    render_attention(f, rows[0], hosts, &attention, theme);
    if card_height > 0 {
        render_health_cards(f, rows[2], &attention, theme);
    }
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(2),
            Constraint::Percentage(50),
        ])
        .split(rows[4]);
    render_busiest(f, bottom[0], hosts, theme);
    render_reachability(f, bottom[2], hosts, selected, theme);
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

/// One row of the busiest list: a host, and a process or a run of identical
/// siblings on it.
#[derive(Debug)]
pub struct Busiest {
    pub host: String,
    pub proc: crate::monitor::Grouped,
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
    let mut rows: Vec<Busiest> = hosts
        .iter()
        .filter(|h| h.state().recovery().is_none())
        .flat_map(|h| {
            crate::monitor::collapse_siblings(&h.vitals.processes)
                .into_iter()
                .map(|proc| Busiest {
                    host: h.name.clone(),
                    proc,
                })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.proc
            .cpu_pct
            .partial_cmp(&a.proc.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Ties by name, so a fleet at rest does not reshuffle every sweep.
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.proc.name.cmp(&b.proc.name))
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

    // "Busiest" has to mean something is running. `ps` reports %CPU to one
    // decimal, so an idle fleet returns a page of processes at 0.0 that the
    // sort then ranks by name — which reads as twelve busy things when the
    // truth is none of them. Show what is moving; count the rest.
    let active_n = all.iter().filter(|p| p.proc.cpu_pct >= BUSY_FLOOR).count();

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

    // One line for the header, and one held back for the tail count when there
    // is a tail to report — a note that scrolls off says nothing.
    let body = inner.height.saturating_sub(1) as usize;
    let shown = if active_n < all.len() || active_n > body {
        body.saturating_sub(1)
    } else {
        body
    }
    .min(active_n);

    if all.is_empty() {
        lines.push(d::none_line("no process readings yet", theme));
    } else if active_n == 0 {
        // The count belongs on this line rather than in a tail note below it:
        // two lines to say "nothing, out of eighteen" wastes a row on a panel
        // that has none spare, and reads as though the eighteen are elsewhere.
        lines.push(d::none_line(
            &format!("nothing above 0.1% cpu, of {} processes", all.len()),
            theme,
        ));
    }
    for p in all.iter().take(shown) {
        let mut row = d::fit_left(&p.host, HOST, Style::default().fg(theme.text_secondary));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_left(
            &p.proc.label(proc_w),
            proc_w,
            Style::default().fg(theme.text_primary),
        ));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_right(
            &format!("{:.1}", p.proc.cpu_pct),
            CPU,
            Style::default().fg(theme.bounded_bad(p.proc.cpu_pct)),
        ));
        row.push(Span::raw(" ".repeat(GAP)));
        row.extend(d::fit_right(
            &widgets::format_bytes(p.proc.mem_rss_kb * 1024),
            RSS,
            Style::default().fg(theme.text_muted),
        ));
        lines.push(Line::from(row));
    }
    if let Some(note) = tail_note(all.len() - shown, all.len() - active_n).filter(|_| active_n > 0)
    {
        lines.push(Line::from(Span::styled(
            note,
            Style::default().fg(theme.text_muted),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The cutoff below which a process is not "busy".
///
/// `ps` gives one decimal, so this is the smallest reading that is not zero.
const BUSY_FLOOR: f64 = 0.1;

/// What the list left out, or `None` when it left out nothing.
///
/// A top-N list must not look complete when it is not, and the two reasons a
/// row is missing are different: one is idle, the other did not fit. When both
/// apply, say so, because "18 more" over a panel showing 296% invites the
/// reader to assume eighteen more like it.
fn tail_note(hidden: usize, idle: usize) -> Option<String> {
    match (hidden, idle) {
        (0, _) => None,
        (h, 0) => Some(format!("{h} more")),
        (h, i) if h == i => Some(format!("{h} more below 0.1%")),
        (h, i) => Some(format!("{h} more, {i} of them below 0.1%")),
    }
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

/// Screen 4 — every binding, and where the state lives. SPEC §3.5.
///
/// `2fr 1fr`, not the handoff's `1.25fr 1fr`. At that ratio the description
/// column is 24 cells and its longest entry, `quit (sessions keep running)`,
/// is 28 — which is defect 4 of the v0.3.3 list, help wrapping mid-phrase,
/// rebuilt at a different width. `keymap::the_column_budget_clears_the_longest_binding`
/// fails the build rather than leaving that to be noticed in a screenshot.
fn render_config_tab(f: &mut Frame, area: Rect, theme: &Theme) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(2, 3),
            Constraint::Length(2),
            Constraint::Min(24),
        ])
        .split(area);
    render_keys_panel(f, cols[0], theme);
    render_state_panel(f, cols[2], theme);
}

/// Panel 1 — everything on one screen, in two labelled columns.
fn render_keys_panel(f: &mut Frame, area: Rect, theme: &Theme) {
    use super::keymap;

    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("keys"),
            sub: Some("everything on one screen"),
            foot_left: &[("t", " cycle theme"), ("^P", " menu")],
            ..Default::default()
        },
    );
    if inner.width < 24 || inner.height < 4 {
        return;
    }

    let mut lines = vec![Line::from(vec![
        Span::styled(
            "In the dashboard, single keys. Inside a session, ",
            Style::default().fg(theme.text_secondary),
        ),
        Span::styled("Ctrl+A", Style::default().fg(theme.key_hint)),
        Span::styled(
            " first — release, then the key.",
            Style::default().fg(theme.text_secondary),
        ),
    ])];
    lines.push(Line::raw(""));

    // The function row, in whole cells. A cell is indivisible, so a break can
    // only ever fall between bindings — which is what stopped `F4 forwards`
    // rendering as `F4 /` then `forwards`, a binding that does not exist,
    // printed by the screen whose job is to teach the bindings.
    lines.extend(key_grid(
        &keymap::FUNCTION_KEYS,
        inner.width as usize,
        3,
        theme,
    ));
    lines.push(Line::raw(""));

    // Two labelled columns side by side, each a stack of whole rows. Every
    // binding is its own cell, so no label wraps mid-phrase.
    let gap = 2usize;
    let col_w = (inner.width as usize).saturating_sub(gap) / 2;
    let left = column_rows(&keymap::IN_SESSION, col_w, theme);
    let right = column_rows(&keymap::DASHBOARD, col_w, theme);

    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<col_w$}", "IN A SESSION"),
            Style::default().fg(theme.text_muted),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled("DASHBOARD", Style::default().fg(theme.text_muted)),
    ]));
    for i in 0..left.len().max(right.len()) {
        let mut spans: Vec<Span> = Vec::new();
        match left.get(i) {
            Some(row) => spans.extend(row.clone()),
            None => spans.push(Span::raw(" ".repeat(col_w))),
        }
        spans.push(Span::raw(" ".repeat(gap)));
        if let Some(row) = right.get(i) {
            spans.extend(row.clone());
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// One labelled column: `key` in its own field, description in the rest.
fn column_rows(
    binds: &[super::keymap::Bind],
    width: usize,
    theme: &Theme,
) -> Vec<Vec<Span<'static>>> {
    let key_w = super::keymap::key_width(binds);
    let desc_w = width.saturating_sub(key_w + 2);
    binds
        .iter()
        .map(|(k, desc)| {
            let mut row = d::fit_left(k, key_w, Style::default().fg(theme.key_hint));
            row.push(Span::raw("  "));
            row.extend(d::fit_left(
                desc,
                desc_w,
                Style::default().fg(theme.text_secondary),
            ));
            row
        })
        .collect()
}

/// A grid of whole `key label` cells, `cols` across.
fn key_grid(
    binds: &[super::keymap::Bind],
    width: usize,
    cols: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let cell = width / cols.max(1);
    binds
        .chunks(cols.max(1))
        .map(|chunk| {
            let mut spans: Vec<Span> = Vec::new();
            for (k, desc) in chunk {
                let mut one = d::fit_left(k, 3, Style::default().fg(theme.key_hint));
                one.push(Span::raw(" "));
                one.extend(d::fit_left(
                    desc,
                    cell.saturating_sub(4),
                    Style::default().fg(theme.text_secondary),
                ));
                spans.extend(one);
            }
            Line::from(spans)
        })
        .collect()
}

/// Panel 2 — where the state lives, and what it says about itself.
fn render_state_panel(f: &mut Frame, area: Rect, theme: &Theme) {
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("2"),
            title: Some("config"),
            sub: Some("~/.essh"),
            foot_left: &[("e", " edit"), ("t", " theme"), ("o", " reveal")],
            ..Default::default()
        },
    );
    if inner.width < 16 || inner.height == 0 {
        return;
    }

    // Two lines per entry rather than one truncated one. `config.toml · 11
    // hosts · edited 4h ago` is 38 cells in a column the layout gives 34, and
    // clipping it loses the half that changes.
    const LABEL: usize = 8;
    let value_w = (inner.width as usize).saturating_sub(LABEL + 1);
    let mut lines: Vec<Line> = Vec::new();
    let mut entry = |label: &str, value: String, note: Option<String>| {
        let mut row = d::fit_left(label, LABEL, Style::default().fg(theme.text_muted));
        row.push(Span::raw(" "));
        row.extend(d::fit_left(
            &value,
            value_w,
            Style::default().fg(theme.text_primary),
        ));
        lines.push(Line::from(row));
        if let Some(note) = note {
            let mut cont = vec![Span::raw(" ".repeat(LABEL + 1))];
            cont.extend(d::fit_left(
                &note,
                value_w,
                Style::default().fg(theme.text_muted),
            ));
            lines.push(Line::from(cont));
        }
    };

    entry("hosts", "config.toml".into(), None);
    entry("cache", "cache.db".into(), None);
    entry("audit", "audit.log".into(), None);
    entry("theme", theme.name.to_string(), None);
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Edits to config.toml reload live — no restart.",
        Style::default().fg(theme.text_muted),
    )));

    f.render_widget(Paragraph::new(lines), inner);
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
        super::DashboardTab::Hosts | super::DashboardTab::Health => &[
            ("m", "monitor"),
            ("a", "add"),
            ("e", "edit"),
            ("/", "search"),
            ("F1", "help"),
            ("^P", "menu"),
            ("q", "quit"),
        ],
        _ => &[
            ("⏎", "connect"),
            ("F1", "help"),
            ("^P", "menu"),
            ("q", "quit"),
        ],
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

    // The panel idiom, like the palette and the divergence card. This was the
    // last square-cornered `Borders::ALL` box in the app: three overlays, two
    // of them rounded, was a difference a reader would take for meaning.
    let surface_style = Style::default().fg(theme.text_primary).bg(theme.panel_alt);
    let title = if editing { "edit host" } else { "add host" };
    let hint = if editing {
        "Update user@host[:port] or host[:port] for the selected host."
    } else {
        "Enter user@host[:port] or host[:port]."
    };

    f.render_widget(Block::default().style(surface_style), popup);
    let inner = d::pane(
        f,
        popup,
        theme,
        &d::PanelOpts {
            title: Some(title),
            focused: true,
            foot_left: &[("⏎", " save"), ("⎋", " cancel")],
            ..Default::default()
        },
    );
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

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
mod hosts_tests {
    use super::*;
    use crate::tui::{HostState, HostStatus, HostVitals};

    /// A panel that shows two busy processes and hides sixteen idle ones must
    /// say so. It must not say "16 more" over a row reading 296%, which invites
    /// the reader to assume sixteen more like it.
    #[test]
    fn the_tail_note_separates_idle_from_clipped() {
        assert_eq!(tail_note(0, 0), None);
        assert_eq!(tail_note(0, 4), None, "nothing hidden, nothing said");
        assert_eq!(tail_note(16, 16).unwrap(), "16 more below 0.1%");
        assert_eq!(tail_note(3, 0).unwrap(), "3 more");
        assert_eq!(tail_note(9, 6).unwrap(), "9 more, 6 of them below 0.1%");
    }

    /// The floor is the smallest reading `ps` can report that is not zero. Set
    /// it higher and a shell doing real work at 0.4% disappears; set it to zero
    /// and the panel is a page of processes tied at nothing.
    #[test]
    fn the_busy_floor_keeps_the_smallest_non_zero_ps_reading() {
        // A real sweep of the fixture fleet: one load generator, one checksum
        // run, a shell that is doing something, and forty-odd idle sshd rows.
        let readings = [296.8_f64, 6.0, 0.4, 0.1, 0.0, 0.0];
        let kept: Vec<f64> = readings
            .iter()
            .copied()
            .filter(|c| *c >= BUSY_FLOOR)
            .collect();
        assert_eq!(kept, vec![296.8, 6.0, 0.4, 0.1]);
    }

    fn host(name: &str, status: HostStatus) -> super::super::HostDisplay {
        super::super::HostDisplay {
            name: name.into(),
            hostname: "127.0.0.20".into(),
            port: 2222,
            user: "matt".into(),
            status,
            last_seen: String::new(),
            tags: String::new(),
            tag_pairs: vec![("env".into(), "prod".into())],
            latency_ms: None,
            latency_history: Vec::new(),
            jump_host: None,
            diverge_count: None,
            vitals: HostVitals::default(),
            session_count: 0,
        }
    }

    /// SPEC §3.2. The handoff's own explanation is 65 cells inside a 64-cell
    /// span, and hand-trimming it only moves the problem to the next width.
    /// So the assertion is about the floor, not the string: whatever else is
    /// dropped, the row says what happened and which key fixes it.
    #[test]
    fn an_unreachable_row_always_states_its_recovery_key() {
        let mut dropped = host("edge-01", HostStatus::Dropped);
        dropped.last_seen = "2026-09-04T11:00:00Z".into();
        dropped.vitals.cpu_pct = Some(9.0);
        dropped.vitals.mem_pct = Some(41.0);
        let offline = host("staging-web-01", HostStatus::Offline);

        for h in [&dropped, &offline] {
            let key = h.state().recovery().expect("a recovery key");
            // Every width from the floor up to a full-width span, so this
            // holds at the terminal sizes `render_smoke` sweeps rather than
            // at the one the reference was drawn at.
            for budget in 20..=90 {
                let reason = unreachable_reason(h, budget);
                assert!(
                    reason.contains(key),
                    "{} at {budget} cells dropped its recovery key: {reason:?}",
                    h.name
                );
                assert!(
                    reason.starts_with("connection reset") || reason.starts_with("offline"),
                    "{} at {budget} cells does not say what happened: {reason:?}",
                    h.name
                );
            }
        }
    }

    /// A host that never answered has no last reading, so it must not carry
    /// one. Inventing `last cpu 0%` for it is the defect SPEC §1 exists to
    /// stop, in the one place where a plausible-looking number is easiest to
    /// reach for.
    #[test]
    fn a_host_that_never_answered_reports_no_last_reading() {
        let mut offline = host("staging-web-01", HostStatus::Offline);
        offline.vitals.cpu_pct = Some(0.0);
        offline.vitals.mem_pct = Some(0.0);
        let reason = unreachable_reason(&offline, 90);
        assert!(!reason.contains("last cpu"), "{reason:?}");
    }

    /// SPEC §3.2's budget, and the reason it is written down: CPU at the
    /// handoff's `8ch` leaves three cells for the meter after the
    /// percentage, and three cells cannot express a ramp.
    #[test]
    fn the_hosts_budget_leaves_a_meter_worth_drawing() {
        let cols = HostCols::fit(146);
        assert_eq!((cols.cpu, cols.mem, cols.disk), (12, 13, 13));
        for w in [cols.cpu, cols.mem, cols.disk] {
            assert!(
                w.saturating_sub(5) >= 7,
                "a {w}-cell column leaves {} cells of meter",
                w.saturating_sub(5)
            );
        }
        assert!(cols.name >= 14, "`staging-web-01` does not fit NAME");
        assert!(cols.tags > 0, "TAGS took no width at all");
    }

    /// Every row is exactly as wide as the panel, at every width the smoke
    /// tests sweep. A row one cell over its budget pushes the column after it
    /// off the screen, and the table looks fine until the one host with a
    /// long name arrives.
    #[test]
    fn every_row_fills_its_width_exactly() {
        let t = crate::theme::dark();
        let hosts = [
            {
                let mut h = host("web-01", HostStatus::Online);
                h.vitals.cpu_pct = Some(12.0);
                h.vitals.mem_pct = Some(46.0);
                h.vitals.disk_pct = Some(81.0);
                h.latency_ms = Some(0.4);
                h.latency_history = vec![1, 2, 3, 2, 1];
                h
            },
            host("staging-web-01", HostStatus::Offline),
            host("edge-01", HostStatus::Dropped),
            host("brand-new-host", HostStatus::NeverProbed),
        ];

        for width in [60usize, 80, 100, 146, 200] {
            let cols = HostCols::fit(width);
            let header: String = cols.header(&t).iter().map(|s| s.content.as_ref()).collect();
            assert!(
                d::width_of(&header) <= width,
                "the header is {} cells at width {width}",
                d::width_of(&header)
            );
            for h in &hosts {
                for selected in [false, true] {
                    let row: String = cols
                        .row(h, selected, &t)
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    assert!(
                        d::width_of(&row) <= width,
                        "{} is {} cells at width {width}: {row:?}",
                        h.name,
                        d::width_of(&row)
                    );
                }
            }
        }
    }

    /// Defect 7 of the v0.3.3 list: eleven of fifteen process rows were
    /// identical nginx workers, so the panel spent its height saying one
    /// thing eleven times.
    #[test]
    fn identical_siblings_collapse_with_their_count() {
        let mut h = host("web-02", HostStatus::Online);
        h.status = HostStatus::Online;
        h.vitals.processes = (0..6)
            .map(|i| crate::monitor::ProcessInfo {
                pid: 100 + i,
                name: "nginx: worker process".into(),
                cpu_pct: 0.5,
                mem_pct: 0.1,
                mem_rss_kb: 3000,
                state: "S".into(),
                tty: "?".into(),
            })
            .collect();
        h.vitals.processes.push(crate::monitor::ProcessInfo {
            pid: 99,
            name: "nginx: master process".into(),
            cpu_pct: 3.1,
            mem_pct: 0.1,
            mem_rss_kb: 1700,
            state: "S".into(),
            tty: "?".into(),
        });

        let rows = busiest(&[h]);
        assert_eq!(rows.len(), 2, "siblings did not collapse: {rows:?}");
        let workers = &rows
            .iter()
            .find(|r| r.proc.name.contains("worker"))
            .expect("the worker row")
            .proc;
        assert_eq!(workers.siblings, 6);
        assert_eq!(workers.mem_rss_kb, 18_000, "RSS is not summed");
        assert!((workers.cpu_pct - 3.0).abs() < 0.001, "CPU is not summed");
        assert!(workers.label(40).ends_with("×6"), "{:?}", workers.label(40));
        // The count survives a column too narrow for the name, because it is
        // reserved before the name is cut.
        assert!(workers.label(12).ends_with("×6"), "{:?}", workers.label(12));
        assert!(d::width_of(&workers.label(12)) <= 12);
    }

    /// An unreachable host's last process list is a snapshot of what *was*
    /// running. "Busiest right now" is a claim about now.
    #[test]
    fn an_unreachable_host_is_left_out_of_busiest() {
        let mut gone = host("edge-01", HostStatus::Dropped);
        gone.vitals.processes = vec![crate::monitor::ProcessInfo {
            pid: 1,
            name: "was-running".into(),
            cpu_pct: 99.0,
            mem_pct: 1.0,
            mem_rss_kb: 1000,
            state: "R".into(),
            tty: "?".into(),
        }];
        assert!(busiest(&[gone]).is_empty());
    }

    /// SPEC §1 rule 3's whole point: a busy host with a named cause is not an
    /// incident, so the row has to carry the cause.
    #[test]
    fn an_attention_row_names_the_process_responsible() {
        let mut busy = host("build-01", HostStatus::Online);
        busy.vitals.cpu_pct = Some(94.0);
        busy.vitals.cpu_over_since =
            std::time::Instant::now().checked_sub(std::time::Duration::from_secs(300));
        busy.vitals.processes = vec![crate::monitor::ProcessInfo {
            pid: 1,
            name: "rustc".into(),
            cpu_pct: 88.2,
            mem_pct: 4.0,
            mem_rss_kb: 2_100_000,
            state: "R".into(),
            tty: "?".into(),
        }];
        assert_eq!(busy.state(), HostState::Busy);
        let (verdict, why) = attention_wording(&busy);
        assert_eq!(verdict, "cpu 94%");
        assert!(why.contains("rustc"), "no cause named: {why:?}");
        assert!(why.contains("over 90%"), "no threshold named: {why:?}");
    }
}

#[cfg(test)]
mod sessions_tests {
    use super::*;
    use crate::tui::HostState;

    fn row(number: usize, last: Option<&str>) -> SessionRow {
        SessionRow {
            number,
            host: "staging-web-01".into(),
            state: "detached",
            host_state: HostState::Up,
            running: Some("cargo build".into()),
            up: "11m".into(),
            cpu_pct: Some(89.3),
            last_line: last.map(|s| s.to_string()),
            idle: "1s".into(),
            dropped: false,
            absent: "—",
        }
    }

    /// The header has to sit over its columns. The `#` column is followed by
    /// a gap, the dot the selection marker replaces, and another gap; missing
    /// the second put every header two cells left of the values under it.
    #[test]
    fn the_header_lines_up_with_its_rows() {
        let t = crate::theme::dark();
        for width in [60usize, 100, 146] {
            let cols = SessionCols::fit(width);
            let header: String = cols.header(&t).iter().map(|s| s.content.as_ref()).collect();
            let body: String = cols
                .row(&row(1, Some("Compiling ratatui")), false, &t)
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            // Display cells, not byte offsets: the state glyph is three
            // bytes and one cell, so `find` disagrees with the screen by two
            // for every row that has one.
            let cells_before = |s: &str, needle: &str| {
                d::width_of(&s[..s.find(needle).expect("column not found")])
            };
            let host_at = cells_before(&header, "HOST");
            let name_at = cells_before(&body, "staging-web-01");
            assert_eq!(
                host_at, name_at,
                "at width {width} the HOST header is at cell {host_at} and its value at {name_at}"
            );
        }
    }

    /// Every row exactly as wide as the panel, at every width the smoke tests
    /// sweep, selected or not.
    #[test]
    fn every_session_row_fills_its_width_exactly() {
        let t = crate::theme::dark();
        let rows = [
            row(1, Some("Compiling ratatui v0.29.0")),
            row(2, None),
            SessionRow {
                dropped: true,
                cpu_pct: None,
                running: None,
                ..row(3, Some("stale"))
            },
        ];
        for width in [50usize, 60, 80, 146, 200] {
            let cols = SessionCols::fit(width);
            let header: String = cols.header(&t).iter().map(|s| s.content.as_ref()).collect();
            assert!(
                d::width_of(&header) <= width,
                "header is {} cells at {width}",
                d::width_of(&header)
            );
            for r in &rows {
                for selected in [false, true] {
                    let line: String = cols
                        .row(r, selected, &t)
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    assert!(
                        d::width_of(&line) <= width,
                        "row {} is {} cells at {width}: {line:?}",
                        r.number,
                        d::width_of(&line)
                    );
                }
            }
        }
    }

    /// LAST LINE is what the screen is for, so it is the column that keeps
    /// its width while the others give way.
    #[test]
    fn last_line_keeps_the_room_as_the_panel_narrows() {
        let wide = SessionCols::fit(146);
        let narrow = SessionCols::fit(70);
        assert!(wide.last > 40, "LAST LINE is {} at 146 cells", wide.last);
        assert!(
            narrow.last >= 12,
            "LAST LINE collapsed to {} at 70 cells",
            narrow.last
        );
        assert!(
            narrow.up == 0 || narrow.cpu == 0,
            "a narrow panel kept every column and squeezed LAST LINE instead"
        );
    }

    /// A dropped session has no live process tree. SPEC §2.2 shows `—` for
    /// its CPU and puts the recovery key in the line column, because a stale
    /// percentage in a live column is the defect the state model exists for.
    #[test]
    fn a_dropped_session_shows_recovery_not_a_stale_reading() {
        let t = crate::theme::dark();
        let cols = SessionCols::fit(146);
        let dropped = SessionRow {
            dropped: true,
            cpu_pct: None,
            running: None,
            state: "dropped",
            ..row(6, Some("this was the last thing it said"))
        };
        let line: String = cols
            .row(&dropped, false, &t)
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(line.contains("r reconnect"), "{line:?}");
        assert!(line.contains("scrollback kept"), "{line:?}");
        assert!(
            !line.contains('%'),
            "a dropped session reported a CPU figure"
        );
    }
}
