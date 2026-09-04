//! The one line of chrome every screen shares. SPEC §4.2 and §1.
//!
//! Two things live here and nowhere else.
//!
//! The **active tab is derived**, never passed in. The v2 design maps six
//! screens onto four tabs: terminal and files both belong to `1 sessions`,
//! help belongs to `4 config`. Passing the tab in means every call site has to
//! remember that mapping, and the first one that forgets renders a screen with
//! no tab lit and no test to catch it.
//!
//! The **header's state counts are a partition** of the host list. They come
//! from `fleet_counts`, which is total over `HostState`, so they sum to the
//! host count by construction rather than by the writer's arithmetic. The v2
//! handoff's three counts sum to eleven only by filing a host whose connection
//! reset six minutes ago under "up", which is the defect this shape makes
//! unrepresentable.

use ratatui::prelude::*;

use super::{fleet_counts, AppView, DashboardTab, HostDisplay};
use crate::theme::Theme;

fn span_width(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}

/// Which tab lights, for a given screen. SPEC's global header.
pub fn active_tab(view: AppView, tab: DashboardTab) -> DashboardTab {
    match view {
        AppView::Dashboard => tab,
        // A session, its monitor, its files and its forwards are all things
        // you do to a session, so the sessions tab stays lit while you are in
        // one. Coming back with `1` lands where you left.
        AppView::Session
        | AppView::Monitor
        | AppView::PortForwarding
        | AppView::FileBrowser
        | AppView::Launcher => DashboardTab::Sessions,
    }
}

/// The tab strip's labels, lowercase per the design.
pub const TABS: [(&str, &str, DashboardTab); 4] = [
    ("1", "sessions", DashboardTab::Sessions),
    ("2", "hosts", DashboardTab::Hosts),
    ("3", "health", DashboardTab::Health),
    ("4", "config", DashboardTab::Config),
];

/// The global header, one line.
///
/// v0.3.3 spent three rows here: a centred title, a tab strip, and an
/// underline row that was blank under every inactive tab. The screen tour
/// called out the dead band it left, and the fix is not a smaller band. The
/// active tab is marked by weight and a background, which costs no row at all.
pub fn header(
    view: AppView,
    tab: DashboardTab,
    hosts: &[HostDisplay],
    session_count: usize,
    width: u16,
    t: &Theme,
) -> Line<'static> {
    let active = active_tab(view, tab);

    let tab_style = |on: bool| {
        if on {
            Style::default()
                .fg(t.text_emphasis)
                .bg(t.panel_alt)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text_muted)
        }
    };

    // Three widths, tried widest first. Below 60 columns the dashboard is at
    // its floor anyway (SPEC §3.6), but the header still has to fit, because
    // `render_smoke` draws every view at every size and a `Line` wider than
    // its `Rect` is a wrapped header rather than a clipped one.
    let brand = |v: &mut Vec<Span>| {
        v.push(Span::raw(" "));
        v.push(Span::styled(
            "ESSH",
            Style::default().fg(t.brand).add_modifier(Modifier::BOLD),
        ));
        v.push(Span::raw("  "));
    };
    let mut left: Vec<Span> = Vec::new();
    brand(&mut left);
    for (n, label, which) in TABS {
        left.push(Span::styled(
            format!(" {n} {label} "),
            tab_style(which == active),
        ));
    }
    if span_width(&left) > width as usize {
        left.clear();
        brand(&mut left);
        for (n, _, which) in TABS {
            left.push(Span::styled(format!(" {n} "), tab_style(which == active)));
        }
    }
    if span_width(&left) > width as usize {
        left.clear();
        for (n, _, which) in TABS {
            left.push(Span::styled(n.to_string(), tab_style(which == active)));
        }
    }

    let mut right: Vec<Span> = Vec::new();
    for (state, n) in fleet_counts(hosts) {
        // Zeroes are dropped. A header that says `0 dropped` invites the
        // reader to go looking for what dropped.
        if n == 0 {
            continue;
        }
        right.push(Span::styled(
            state.glyph(),
            Style::default().fg(state.color(t)),
        ));
        right.push(Span::styled(
            format!(" {n} {}  ", state.word()),
            Style::default().fg(t.text_secondary),
        ));
    }
    if !hosts.is_empty() {
        right.push(Span::styled("│  ", Style::default().fg(t.separator)));
    }
    right.push(Span::styled(
        crate::format::count(session_count, "session"),
        Style::default().fg(t.text_secondary),
    ));
    right.push(Span::styled(
        format!("  {} ", chrono::Local::now().format("%H:%M:%S")),
        Style::default().fg(t.text_muted),
    ));

    let pad = (width as usize).saturating_sub(span_width(&left) + span_width(&right));

    // Narrow enough and the counts would overwrite the tabs. Dropping the
    // right-hand block whole is the honest degradation: half a partition
    // reads as a wrong one.
    let mut out = left;
    if pad > 0 {
        out.push(Span::raw(" ".repeat(pad)));
        out.extend(right);
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{HostState, HostStatus, HostVitals};

    fn host(name: &str, status: HostStatus, vitals: HostVitals) -> HostDisplay {
        HostDisplay {
            name: name.into(),
            hostname: format!("127.0.0.{}", name.len()),
            port: 2222,
            user: "matt".into(),
            status,
            last_seen: String::new(),
            tags: String::new(),
            tag_pairs: Vec::new(),
            latency_ms: None,
            latency_history: Vec::new(),
            jump_host: None,
            diverge_count: None,
            vitals,
            session_count: 0,
        }
    }

    fn busy_disk() -> HostVitals {
        HostVitals {
            disk_pct: Some(81.0),
            ..Default::default()
        }
    }

    /// SPEC §1, and the finding that produced it.
    ///
    /// The v2 handoff's header reads `9 up · 1 busy · 1 offline` over eleven
    /// hosts. It sums, but only because the host whose connection reset six
    /// minutes ago is counted as up on the same screen that explains the
    /// reset. A partition cannot do that.
    #[test]
    fn the_header_counts_partition_the_fleet() {
        let hosts = vec![
            host("web-01", HostStatus::Online, HostVitals::default()),
            host("db-01", HostStatus::Online, busy_disk()),
            host("edge-01", HostStatus::Dropped, HostVitals::default()),
            host("staging", HostStatus::Offline, HostVitals::default()),
            host("new-01", HostStatus::NeverProbed, HostVitals::default()),
        ];

        let counts = fleet_counts(&hosts);
        assert_eq!(counts.iter().map(|(_, n)| n).sum::<usize>(), hosts.len());
        assert_eq!(
            counts.map(|(s, n)| (s, n)),
            [
                (HostState::Up, 1),
                (HostState::Busy, 1),
                (HostState::Dropped, 1),
                (HostState::Offline, 1),
                (HostState::Unprobed, 1),
            ]
        );
    }

    /// A host is in exactly one state whatever its readings say, so the sum
    /// holds for any fleet rather than for the one the test happened to pick.
    #[test]
    fn every_host_lands_in_exactly_one_state() {
        let statuses = [
            HostStatus::Online,
            HostStatus::Dropped,
            HostStatus::Offline,
            HostStatus::NeverProbed,
        ];
        let readings = [
            HostVitals::default(),
            busy_disk(),
            HostVitals {
                mem_pct: Some(97.0),
                ..Default::default()
            },
            HostVitals {
                cpu_pct: Some(99.0),
                ..Default::default()
            },
        ];
        let mut hosts = Vec::new();
        for s in &statuses {
            for v in &readings {
                hosts.push(host("h", s.clone(), v.clone()));
            }
        }
        assert_eq!(
            fleet_counts(&hosts).iter().map(|(_, n)| n).sum::<usize>(),
            hosts.len()
        );
    }

    /// CPU is the one sustained threshold. A host that has just crossed 90%
    /// is not busy yet, or every compile flaps the fleet count.
    #[test]
    fn a_cpu_spike_does_not_flap_the_count() {
        let spiking = host(
            "build-01",
            HostStatus::Online,
            HostVitals {
                cpu_pct: Some(99.0),
                cpu_over_since: Some(std::time::Instant::now()),
                ..Default::default()
            },
        );
        assert_eq!(spiking.state(), HostState::Up);

        let sustained = host(
            "build-01",
            HostStatus::Online,
            HostVitals {
                cpu_pct: Some(99.0),
                cpu_over_since: std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(300)),
                ..Default::default()
            },
        );
        assert_eq!(sustained.state(), HostState::Busy);
    }

    /// The two unreachable states are not the same unreachable, and the row
    /// has to offer the right recovery for each.
    #[test]
    fn dropped_and_offline_offer_different_recovery() {
        assert_eq!(HostState::Dropped.recovery(), Some("r reconnect"));
        assert_eq!(HostState::Offline.recovery(), Some("r retry"));
        assert_eq!(HostState::Up.recovery(), None);
        assert_eq!(HostState::Unprobed.recovery(), None);
    }

    /// The mapping the handoff states in prose and no call site should hold
    /// a copy of.
    #[test]
    fn the_active_tab_is_derived_from_the_screen() {
        for view in [
            AppView::Session,
            AppView::Monitor,
            AppView::FileBrowser,
            AppView::PortForwarding,
        ] {
            assert_eq!(
                active_tab(view, DashboardTab::Hosts),
                DashboardTab::Sessions,
                "{view:?} belongs to the sessions tab"
            );
        }
        assert_eq!(
            active_tab(AppView::Dashboard, DashboardTab::Health),
            DashboardTab::Health
        );
    }

    /// One line, whatever the fleet. The three-row header is the dead band
    /// the v0.3.3 screen tour complained about.
    #[test]
    fn the_header_is_one_line_and_fits_its_width() {
        let t = crate::theme::dark();
        let hosts: Vec<_> = (0..40)
            .map(|i| host(&format!("h{i}"), HostStatus::Online, HostVitals::default()))
            .collect();
        for width in [40u16, 80, 150, 200] {
            let line = header(
                AppView::Dashboard,
                DashboardTab::Hosts,
                &hosts,
                9,
                width,
                &t,
            );
            let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let cells = unicode_width::UnicodeWidthStr::width(rendered.as_str());
            assert!(
                cells <= width as usize,
                "header overflows at {width}: {cells} cells"
            );
            assert!(!rendered.contains('\n'));
        }
    }
}
