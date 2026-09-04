use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::design as d;
use crate::filetransfer::{FileBrowser, FilePaneFocus};
use crate::theme::Theme;
use crate::tui::widgets;

pub fn render(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    f.render_widget(Clear, area);

    // Main layout: panes area + transfer bar + footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),    // two-pane file listing
            Constraint::Length(2), // transfer progress
            Constraint::Length(2), // footer keybindings
        ])
        .split(area);

    // Split panes horizontally
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    render_local_pane(f, panes[0], browser, theme);
    render_remote_pane(f, panes[1], browser, theme);
    render_transfer_bar(f, chunks[1], browser, theme);
    render_footer(f, chunks[2], theme);
}

/// Names that exist in both panes. SPEC §2.6.
///
/// Marked on both sides, so the conflict is visible from whichever pane you
/// are about to act in. Copying onto a name that is already there is the one
/// mistake a two-pane browser makes easy, and it makes it silently.
///
/// The file currently being transferred is excluded: a partial upload of the
/// file you are uploading is not a conflict, and colouring it as one is the
/// browser warning you about itself.
fn collisions(browser: &FileBrowser) -> std::collections::HashSet<&str> {
    let remote: std::collections::HashSet<&str> = browser
        .remote_files
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name.as_str())
        .collect();
    let in_flight = browser.transfer.as_ref().map(|t| t.filename.as_str());
    browser
        .local_files
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name.as_str())
        .filter(|n| remote.contains(n) && Some(*n) != in_flight)
        .collect()
}

fn render_local_pane(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let is_active = browser.focus == FilePaneFocus::Local;
    let border_color = if is_active {
        theme.active_tab
    } else {
        theme.brand
    };
    let _ = border_color;
    // The path is the panel's qualifier, tail-first: when it does not fit, the
    // directory you are in matters more than the root you came from.
    let path = browser.local_path.display().to_string();
    let path = elide_left(&path, area.width.saturating_sub(24) as usize);
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("1"),
            title: Some("local"),
            sub: Some(&path),
            focused: is_active,
            foot_left: &[("↑↓", " select"), ("↵", " open"), ("⌫", " up")],
            ..Default::default()
        },
    );

    let clashing = collisions(browser);
    let visible_height = inner.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(visible_height);

    // Parent directory entry
    lines.push(Line::from(Span::styled(
        "  ..",
        Style::default().fg(theme.text_muted),
    )));

    for (i, entry) in browser.local_files.iter().enumerate() {
        if lines.len() >= visible_height {
            break;
        }
        let is_selected = i == browser.local_selected && is_active;
        let base = if entry.is_dir {
            theme.status_info
        } else if clashing.contains(entry.name.as_str()) {
            theme.status_warn
        } else {
            theme.text_primary
        };
        let style = if is_selected {
            // Dark fill plus a caret, not an inverted bar: the row keeps its
            // own colour, so a directory still reads as a directory — and a
            // collision still reads as a collision — when it is selected.
            Style::default().fg(base).bg(theme.selection_bg).bold()
        } else {
            Style::default().fg(base)
        };

        let display_name = if entry.is_dir {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };

        let size_str = if entry.is_dir {
            String::new()
        } else {
            widgets::format_bytes(entry.size)
        };

        // Saturating: in release this subtraction wraps rather than panics,
        // and a wrapped usize as a format width asks `format!` to build a
        // string of ~2^64 spaces — the pane goes unresponsive and the process
        // grows until it is killed. A narrow pane must degrade, not hang.
        let name_width = (inner.width as usize)
            .saturating_sub(size_str.len())
            .saturating_sub(4);
        let marker = if is_selected { "▶ " } else { "  " };
        let padded = format!(
            "{marker}{:<width$}{}",
            display_name,
            size_str,
            width = name_width
        );
        lines.push(Line::from(Span::styled(padded, style)));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn render_remote_pane(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let is_active = browser.focus == FilePaneFocus::Remote;
    let border_color = if is_active {
        theme.active_tab
    } else {
        theme.brand
    };
    let _ = border_color;
    let path = elide_left(&browser.remote_path, area.width.saturating_sub(26) as usize);
    let inner = d::pane(
        f,
        area,
        theme,
        &d::PanelOpts {
            key: Some("2"),
            title: Some("remote"),
            sub: Some(&path),
            focused: is_active,
            foot_left: &[("u", " upload"), ("d", " download"), ("⇥", " switch")],
            ..Default::default()
        },
    );

    let clashing = collisions(browser);
    let visible_height = inner.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(visible_height);

    // Parent directory entry
    lines.push(Line::from(Span::styled(
        "  ..",
        Style::default().fg(theme.text_muted),
    )));

    if browser.remote_files.is_empty() && browser.status_message.is_none() {
        lines.push(Line::from(Span::styled(
            "  (loading...)",
            Style::default().fg(theme.text_muted),
        )));
    }

    for (i, entry) in browser.remote_files.iter().enumerate() {
        if lines.len() >= visible_height {
            break;
        }
        let is_selected = i == browser.remote_selected && is_active;
        let base = if entry.is_dir {
            theme.status_info
        } else if clashing.contains(entry.name.as_str()) {
            theme.status_warn
        } else {
            theme.text_primary
        };
        let style = if is_selected {
            Style::default().fg(base).bg(theme.selection_bg).bold()
        } else {
            Style::default().fg(base)
        };

        let display_name = if entry.is_dir {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };

        let size_str = if entry.is_dir {
            String::new()
        } else {
            widgets::format_bytes(entry.size)
        };

        // Saturating: in release this subtraction wraps rather than panics,
        // and a wrapped usize as a format width asks `format!` to build a
        // string of ~2^64 spaces — the pane goes unresponsive and the process
        // grows until it is killed. A narrow pane must degrade, not hang.
        let name_width = (inner.width as usize)
            .saturating_sub(size_str.len())
            .saturating_sub(4);
        let marker = if is_selected { "▶ " } else { "  " };
        let padded = format!(
            "{marker}{:<width$}{}",
            display_name,
            size_str,
            width = name_width
        );
        lines.push(Line::from(Span::styled(padded, style)));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn render_transfer_bar(f: &mut Frame, area: Rect, browser: &FileBrowser, theme: &Theme) {
    let line = if let Some(ref transfer) = browser.transfer {
        let pct = transfer.percent();
        let arrow = match transfer.direction {
            crate::filetransfer::TransferDirection::Upload => "↑",
            crate::filetransfer::TransferDirection::Download => "↓",
        };
        // SPEC §2.6: rate and ETA, never a bare bar. A percentage says how
        // much is left and nothing about whether it is worth waiting for.
        // Both are absent until there is an interval to divide by, because a
        // rate over zero seconds is a division by zero with a unit on it.
        let tail = match (transfer.rate_bps(), transfer.eta_secs()) {
            (Some(rate), Some(eta)) => format!(
                "{:.0}% · {}/s · {}",
                pct,
                widgets::format_bytes(rate as u64),
                crate::format::short_duration(eta)
            ),
            _ => format!("{pct:.0}% · measuring…"),
        };
        let label = format!(" {arrow} {} ", transfer.filename);
        let bar_width = (area.width as usize)
            .saturating_sub(label.chars().count() + tail.chars().count() + 3)
            .clamp(5, 48);
        let mut spans = vec![Span::styled(label, Style::default().fg(theme.text_primary))];
        spans.extend(d::meter_flat(
            pct / 100.0,
            bar_width,
            theme.status_good,
            theme,
        ));
        spans.push(Span::styled(
            format!("  {tail}"),
            Style::default().fg(theme.text_secondary),
        ));
        Line::from(spans)
    } else if let Some(ref msg) = browser.status_message {
        Line::from(Span::styled(
            format!(" {}", msg),
            Style::default().fg(theme.status_warn),
        ))
    } else {
        Line::from(Span::styled(
            " Ready",
            Style::default().fg(theme.text_muted),
        ))
    };

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border)),
    );
    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut Frame, area: Rect, theme: &Theme) {
    let _ = theme;
    // The navigation and transfer binds live on the panel rules; these are the
    // ones that belong to the screen rather than to a pane.
    let footer = Paragraph::new(d::footer_line(
        &[
            ("m", "mkdir"),
            ("Del", "delete"),
            ("t", "theme"),
            ("⎋", "close"),
        ],
        theme,
    ));
    f.render_widget(footer, area);
}

/// Trim a path from the left, keeping whole segments: `…/deploy/releases/current`.
///
/// Two rules, and the design reference gets one of them wrong.
///
/// Paths are read right-to-left — the leaf is the answer, the root is context
/// — so truncating the tail throws away the half that matters. That much the
/// reference has.
///
/// It cuts with CSS `direction: rtl`, which SPEC §2.6 rejects: that reorders
/// the neutral `/` characters and displays a path that is not the real one.
/// Cutting here is done in the string, before anything is drawn, and on a
/// separator — so `…/releases/current` rather than `…eases/current`, which
/// reads as a directory called `eases` and is not one.
fn elide_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    // Longest whole-segment tail that fits after the ellipsis.
    let budget = max.saturating_sub(1);
    let mut best: Option<&str> = None;
    for (i, _) in s.char_indices().filter(|(_, c)| *c == '/') {
        let tail = &s[i..];
        if tail.chars().count() <= budget {
            best = Some(tail);
            break;
        }
    }
    match best {
        Some(tail) => format!("…{tail}"),
        // No separator fits: fall back to a raw cut rather than showing
        // nothing. A leaf longer than the column is a real case.
        None => {
            let tail: String = s
                .chars()
                .rev()
                .take(budget)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{tail}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// A pane too narrow for the size column must still render.
    ///
    /// The width arithmetic used to underflow, and an underflowed `usize` as a
    /// format width makes `format!` try to allocate an unbounded string —
    /// which presents as the app locking up, not as a crash.
    #[test]
    fn a_very_narrow_pane_does_not_hang() {
        let theme = crate::theme::dark();
        for width in [1u16, 2, 4, 8, 12] {
            let mut term = Terminal::new(TestBackend::new(width, 6)).unwrap();
            let mut browser = FileBrowser::new();
            // A long name plus a size string is what makes the width
            // arithmetic go negative.
            browser.local_files = vec![crate::filetransfer::LocalFileEntry {
                name: "a-file-with-a-long-name.tar.gz".into(),
                path: std::path::PathBuf::from("/tmp/a-file-with-a-long-name.tar.gz"),
                is_dir: false,
                size: 1_234_567,
            }];
            term.draw(|f| render(f, f.area(), &browser, &theme))
                .unwrap();
        }
    }

    #[test]
    fn a_path_is_elided_from_the_left_keeping_the_leaf() {
        // The leaf is the answer; the root is context.
        assert_eq!(elide_left("/a/b/c", 10), "/a/b/c");
        // On a separator, so every segment shown is a real directory name.
        // This used to cut mid-segment and produce `…h/to/here`, which reads
        // as a directory called `h` and is not one.
        assert_eq!(elide_left("/very/long/path/to/here", 10), "…/to/here");
        assert_eq!(elide_left("anything", 0), "");
    }
}

#[cfg(test)]
mod files_v2_tests {
    use super::*;
    use crate::filetransfer::{
        LocalFileEntry, RemoteFileEntry, TransferDirection, TransferProgress,
    };

    fn browser() -> FileBrowser {
        let mut b = FileBrowser::new();
        b.local_files = vec![
            LocalFileEntry {
                name: "app-2.14.4.tar.gz".into(),
                path: "/tmp/app-2.14.4.tar.gz".into(),
                is_dir: false,
                size: 38_800_000,
            },
            LocalFileEntry {
                name: "deploy.sh".into(),
                path: "/tmp/deploy.sh".into(),
                is_dir: false,
                size: 1_200,
            },
            LocalFileEntry {
                name: "notes".into(),
                path: "/tmp/notes".into(),
                is_dir: true,
                size: 0,
            },
        ];
        b.remote_files = vec![
            RemoteFileEntry {
                name: "app-2.14.4.tar.gz".into(),
                is_dir: false,
                size: 20_200_000,
                permissions: "-rw-r--r--".into(),
                modified: String::new(),
            },
            RemoteFileEntry {
                name: "deploy.sh".into(),
                is_dir: false,
                size: 46,
                permissions: "-rwxr-xr-x".into(),
                modified: String::new(),
            },
            RemoteFileEntry {
                name: "notes".into(),
                is_dir: true,
                size: 0,
                permissions: "drwxr-xr-x".into(),
                modified: String::new(),
            },
        ];
        b
    }

    /// SPEC §2.6. A name in both panes is the one mistake a two-pane browser
    /// makes easy, and it makes it silently.
    #[test]
    fn a_name_in_both_panes_is_a_collision() {
        let b = browser();
        let clashing = collisions(&b);
        assert!(clashing.contains("deploy.sh"));
        assert!(clashing.contains("app-2.14.4.tar.gz"));
        // Directories are navigation, not overwrites.
        assert!(!clashing.contains("notes"));
    }

    /// A partial upload of the file you are uploading is not a conflict, and
    /// colouring it as one is the browser warning you about itself.
    #[test]
    fn the_file_being_transferred_is_not_a_collision() {
        let mut b = browser();
        b.transfer = Some(TransferProgress {
            filename: "app-2.14.4.tar.gz".into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 20_200_000,
            total_bytes: 38_800_000,
            complete: false,
            started: None,
        });
        let clashing = collisions(&b);
        assert!(!clashing.contains("app-2.14.4.tar.gz"));
        assert!(
            clashing.contains("deploy.sh"),
            "other collisions still count"
        );
    }

    /// SPEC §2.6 rejects the reference's `direction: rtl` trick because it
    /// reorders the neutral `/` and shows a path that is not the real one.
    /// Cutting in the string, on a separator, keeps every segment real.
    #[test]
    fn a_shortened_path_is_still_a_path() {
        let p = "/home/matt/work/essh-demo/home/Downloads";
        assert_eq!(elide_left(p, 100), p, "a path that fits is left alone");

        let short = elide_left(p, 24);
        assert!(short.chars().count() <= 24, "{short}");
        assert!(short.starts_with('…'), "{short}");
        assert!(
            short.ends_with("/Downloads"),
            "the leaf was cut off: {short}"
        );
        // Every segment after the ellipsis is a real directory name, not the
        // tail end of one.
        for seg in short
            .trim_start_matches('…')
            .split('/')
            .filter(|s| !s.is_empty())
        {
            assert!(
                p.split('/').any(|real| real == seg),
                "`{seg}` is not a segment of the real path: {short}"
            );
        }
    }

    /// A leaf longer than the column still has to render something.
    #[test]
    fn a_leaf_wider_than_the_column_still_shows() {
        let p = "/srv/a-directory-name-far-longer-than-any-sensible-column";
        for max in [1usize, 2, 8, 20] {
            let out = elide_left(p, max);
            assert!(out.chars().count() <= max, "{max}: {out}");
        }
    }

    /// SPEC §2.6: never a bare bar. And never a rate divided by no time.
    #[test]
    fn a_transfer_reports_no_rate_until_it_has_one() {
        let mut t = TransferProgress {
            filename: "app.tar.gz".into(),
            direction: TransferDirection::Upload,
            bytes_transferred: 0,
            total_bytes: 38_800_000,
            complete: false,
            started: Some(std::time::Instant::now()),
        };
        assert_eq!(t.rate_bps(), None, "a rate over zero seconds is not a rate");
        assert_eq!(t.eta_secs(), None);

        t.started = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(6));
        t.bytes_transferred = 20_176_000;
        let rate = t.rate_bps().expect("a rate");
        assert!(
            (rate - 3_362_666.0).abs() < 50_000.0,
            "rate looks wrong: {rate}"
        );
        // 48% of 38.8MB left at that rate is about five and a half seconds.
        let eta = t.eta_secs().expect("an eta");
        assert!((5..=6).contains(&eta), "eta {eta}s");
    }
}
