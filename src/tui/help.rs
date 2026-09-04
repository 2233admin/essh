use ratatui::{
    prelude::*,
    widgets::{Clear, Paragraph, Wrap},
};

use crate::design as d;
use crate::theme::Theme;

/// The one-press bindings, as data.
///
/// A list rather than a hand-spaced string because the grid needs the pairs
/// intact to keep each one on a single row, and the test needs them to check
/// that it did.
/// The function row, from the one place it is written down.
///
/// It used to be a second copy here. That is how the help screen came to
/// teach `r  Refresh hosts` for a release after `r` stopped refreshing
/// anything: two lists, one of which nobody updated.
use super::keymap::FUNCTION_KEYS;

/// Lay `pairs` out as a grid of whole `key label` cells.
///
/// v0.3.3 built the function-key row as one long line and let `Wrap` break it
/// wherever it landed, which produced "F4 /" at the end of one row and
/// "forwards" at the start of the next — a binding that does not exist,
/// printed by the screen whose job is to teach the bindings. A cell is
/// indivisible: the row takes as many as fit and starts a new row for the
/// rest, so the break can only ever fall between bindings.
fn key_grid(
    pairs: &[(&str, &str)],
    width: u16,
    key_style: Style,
    desc_style: Style,
) -> Vec<Line<'static>> {
    const GUTTER: usize = 3;
    const INDENT: usize = 4;

    let cell = pairs
        .iter()
        .map(|(k, d)| k.chars().count() + 1 + d.chars().count())
        .max()
        .unwrap_or(0);
    let usable = (width as usize).saturating_sub(INDENT);
    // At least one cell per row even when the terminal is too narrow for the
    // widest one: a cramped row beats an empty screen.
    let cols = ((usable + GUTTER) / (cell + GUTTER)).max(1);

    pairs
        .chunks(cols)
        .map(|chunk| {
            let mut spans = vec![Span::raw(" ".repeat(INDENT))];
            for (i, (k, d)) in chunk.iter().enumerate() {
                spans.push(Span::styled((*k).to_string(), key_style));
                spans.push(Span::styled(format!(" {}", d), desc_style));
                if i + 1 < chunk.len() {
                    let used = k.chars().count() + 1 + d.chars().count();
                    spans.push(Span::raw(" ".repeat(cell - used + GUTTER)));
                }
            }
            Line::from(spans)
        })
        .collect()
}

pub fn render(f: &mut Frame, theme: &Theme, prefix: &str, scroll: u16) {
    let area = f.area();

    let key_style = Style::default().fg(theme.key_hint).bold();
    let heading_style = Style::default().fg(theme.active_tab).bold();
    let desc_style = Style::default().fg(theme.text_primary);
    let dim = Style::default().fg(theme.text_muted);
    // Shown as the prefix, because that is the binding that works in every
    // terminal on both platforms. See `prefix_hint` for why the Alt/Option
    // form is not advertised per-row.
    let p = |k: &str| format!("    {:<14}", crate::tui::prefix_hint(prefix, k));
    let session_jump_hint = p("1-9");
    let prev_next_hint = p("←/→");
    let last_session_hint = p("Tab");
    let split_hint = p("M");
    let split_resize_hint = p("[/]");
    let monitor_hint = p("m");
    let portfwd_hint = p("p");
    let files_hint = p("f");
    let detach_hint = p("d");
    let new_hint = p("n");
    let close_hint = p("w");
    let theme_hint = p("t");

    // The popup width has to be known before the content is built: the
    // function-key grid sizes its columns to fit, which is the whole reason a
    // binding can no longer be broken in half.
    let popup_width = 68u16.min(area.width.saturating_sub(4));
    let inner_width = popup_width.saturating_sub(2);

    let mut lines: Vec<Line> = vec![
        Line::raw(""),
        Line::styled("  One press, no prefix", heading_style),
    ];
    lines.extend(key_grid(&FUNCTION_KEYS, inner_width, key_style, desc_style));

    lines.extend(vec![
        // Stock macOS maps that row to brightness and volume — F1 dims the
        // screen, F10 mutes. Advertising the keys without saying so sends a
        // Mac user to change their volume and wonder why nothing happened.
        #[cfg(target_os = "macos")]
        Line::from(Span::styled(
            "                         hold fn, or turn on Keyboard → \"Use F1, F2 as standard function keys\"",
            desc_style,
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  For everything else, press ", desc_style),
            Span::styled(crate::tui::prefix_label(prefix), key_style),
            Span::styled(", let go, then the key.", desc_style),
        ]),
        Line::from(Span::styled(
            format!(
                "  {}+key also works if your terminal sends {} as Meta.",
                crate::tui::meta_key_label(),
                crate::tui::meta_key_label()
            ),
            dim,
        )),
        Line::raw(""),
        Line::styled("  In a session", heading_style),
        Line::from(vec![
            Span::styled("    ?           ", key_style),
            Span::styled("Toggle this help menu", desc_style),
        ]),
        Line::from(vec![
            Span::styled(session_jump_hint, key_style),
            Span::styled("Jump to session N", desc_style),
        ]),
        Line::from(vec![
            Span::styled(prev_next_hint, key_style),
            Span::styled("Previous / next session", desc_style),
        ]),
        Line::from(vec![
            Span::styled(last_session_hint, key_style),
            Span::styled("Switch to last session", desc_style),
        ]),
        Line::from(vec![
            Span::styled(split_hint, key_style),
            Span::styled("Toggle split-pane (terminal + monitor)", desc_style),
        ]),
        Line::from(vec![
            Span::styled(split_resize_hint, key_style),
            Span::styled("Adjust split-pane width", desc_style),
        ]),
        Line::from(vec![
            Span::styled(monitor_hint, key_style),
            Span::styled("Toggle host monitor (full-screen)", desc_style),
        ]),
        Line::from(vec![
            Span::styled(portfwd_hint, key_style),
            Span::styled("Toggle port forwarding manager", desc_style),
        ]),
        Line::from(vec![
            Span::styled(files_hint, key_style),
            Span::styled("File browser (upload/download)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    F10         ", key_style),
            Span::styled("Command palette (fuzzy search)", desc_style),
        ]),
        Line::from(vec![
            Span::styled(new_hint, key_style),
            Span::styled("Open the launcher to start another session", desc_style),
        ]),
        Line::from(vec![
            Span::styled(detach_hint, key_style),
            Span::styled("Detach to dashboard", desc_style),
        ]),
        Line::from(vec![
            Span::styled(close_hint, key_style),
            Span::styled("Close active session", desc_style),
        ]),
        Line::from(vec![
            Span::styled(theme_hint, key_style),
            Span::styled("Cycle theme", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Dashboard", heading_style),
        Line::from(vec![
            Span::styled("    1-4         ", key_style),
            Span::styled("Switch tab (Sessions/Hosts/Health/Config)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    j/k ↑/↓     ", key_style),
            Span::styled("Navigate host list", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Enter       ", key_style),
            Span::styled("Connect to selected host", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    a           ", key_style),
            Span::styled("Add host (user@host[:port])", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    r           ", key_style),
            Span::styled("Reconnect or retry the selected host", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Tab         ", key_style),
            Span::styled("Focus the next panel (Sessions)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    R           ", key_style),
            Span::styled("Reload config (keeps measurements)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    /           ", key_style),
            Span::styled("Start host search/filter", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    e           ", key_style),
            Span::styled("Edit host (Hosts) / config (Config)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    t           ", key_style),
            Span::styled("Cycle theme", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    d           ", key_style),
            Span::styled("Delete selected host", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    q / Ctrl+c  ", key_style),
            Span::styled("Quit (sessions keep running)", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Search", heading_style),
        Line::from(vec![
            Span::styled("    type / ⌫    ", key_style),
            Span::styled("Filter hosts as you type", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Enter / Esc ", key_style),
            Span::styled("Connect first match / cancel search", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Command Palette", heading_style),
        Line::from(vec![
            Span::styled("    ↑/↓ Tab     ", key_style),
            Span::styled("Move selection (Shift+Tab moves up)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Enter       ", key_style),
            Span::styled("Run selected action", desc_style),
        ]),
        Line::from(vec![
            // `Esc / Ctrl+p` fills the 12-column key field exactly, so it ran
            // straight into its description: "Esc / Ctrl+pClose palette".
            // `^P` is what the footers call it anyway.
            Span::styled("    Esc / ^P    ", key_style),
            Span::styled("Close palette", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Session", heading_style),
        Line::from(vec![
            Span::styled("    (all keys)  ", key_style),
            Span::styled("Forwarded to remote shell", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  File Browser", heading_style),
        Line::from(vec![
            Span::styled("    ↑/↓ Enter   ", key_style),
            Span::styled("Select and open directories", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Backspace   ", key_style),
            Span::styled("Go to parent directory", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Tab         ", key_style),
            Span::styled("Switch local/remote pane", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    u d m Del   ", key_style),
            Span::styled("Upload, download, mkdir, delete", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Port Forwarding", heading_style),
        Line::from(vec![
            Span::styled("    ↑/↓ a d     ", key_style),
            Span::styled("Select, add, and delete forwards", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    type Enter  ", key_style),
            Span::styled("In add mode, enter a forward spec", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    ⌫ / Esc     ", key_style),
            Span::styled("Edit or cancel add-mode input", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Monitor", heading_style),
        Line::from(vec![
            Span::styled("    s           ", key_style),
            Span::styled("Toggle sort (CPU / Memory)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    ↑/↓         ", key_style),
            Span::styled("Scroll process list", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    Esc         ", key_style),
            Span::styled("Return to terminal", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    t           ", key_style),
            Span::styled("Cycle theme", desc_style),
        ]),
        Line::raw(""),
        Line::styled("  Config", heading_style),
        Line::from(vec![
            Span::styled("    e           ", key_style),
            Span::styled("Open ~/.essh/config.toml and reload", desc_style),
        ]),
        Line::from(vec![
            Span::styled("    notification_patterns", key_style),
            Span::styled("  Background alert regexes", desc_style),
        ]),
        Line::raw(""),
        Line::styled("                    Press ? or Esc to close", dim),
    ]);

    // Size to the content, then to the screen. A fixed cap smaller than the
    // reference clips it even on a tall terminal, which is how the last
    // section became unreachable on a 60-row display.
    // Height from the *wrapped* content, so "tall enough" is measured in rows
    // that will actually be drawn rather than in source lines.
    let wanted = wrapped_height(&lines, popup_width.saturating_sub(2)) + 2;
    let popup_height = wanted.min(area.height.saturating_sub(2)).max(3);
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup = Rect::new(x, y, popup_width, popup_height);
    f.render_widget(Clear, popup);

    // How far the content can scroll before the last line is on screen.
    //
    // Measured *after* wrapping, not from `lines.len()`. A line longer than
    // the popup occupies two rows, so counting source lines under-counts the
    // height and leaves the tail permanently out of reach — with a range
    // indicator that confidently reports the wrong total.
    let body = popup.height.saturating_sub(2);
    let rendered = wrapped_height(&lines, popup.width.saturating_sub(2));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let overflow = rendered.saturating_sub(body);
    let scroll = scroll.min(overflow);
    let more = if overflow > 0 {
        Some(format!(
            "{}–{} of {}",
            scroll + 1,
            (scroll + body).min(rendered),
            rendered
        ))
    } else {
        None
    };

    let inner = d::pane(
        f,
        popup,
        theme,
        &d::PanelOpts {
            title: Some("help"),
            focused: true,
            foot_left: &[("↑↓", " scroll"), ("?/⎋", " close")],
            foot_right: more.as_deref(),
            ..Default::default()
        },
    );
    f.render_widget(paragraph.scroll((scroll, 0)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen(width: u16, height: u16, scroll: u16) -> String {
        let theme = crate::theme::dark();
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| render(f, &theme, "ctrl-a", scroll)).unwrap();
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

    /// `cargo test --bins help::tests::visual_dump -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn visual_dump() {
        println!("\n{}", screen(150, 40, 0));
    }

    /// No key column may touch its description.
    ///
    /// The keymap pads keys into a fixed field by hand, so a label that fills
    /// the field exactly collides with the text after it — which is how
    /// "Esc / Ctrl+pClose palette" reached the screen. One row's worth of
    /// padding is not a thing to check by eye on every edit.
    #[test]
    fn every_binding_is_separated_from_its_description() {
        for width in [70, 90, 120, 150] {
            let s = screen(width, 200, 0);
            for line in s.lines() {
                let line = line.trim_end();
                // A lowercase letter or digit immediately followed by an
                // uppercase word is the collision's signature.
                let chars: Vec<char> = line.chars().collect();
                for w in chars.windows(2) {
                    let collision = (w[0].is_ascii_lowercase() || w[0].is_ascii_digit())
                        && w[1].is_ascii_uppercase();
                    if collision {
                        // Allow real words that contain a capital mid-token.
                        let ok = ["Ctrl", "Esc", "Enter", "Tab", "Del", "Backspace", "Shift"]
                            .iter()
                            .any(|t| {
                                line.contains(&format!("+{t}")) || line.contains(&format!("/{t}"))
                            });
                        assert!(
                            ok,
                            "at {width} cols a key runs into its description:\n{line}"
                        );
                    }
                }
            }
        }
    }

    /// The defect this screen shipped with: the function-key row was one long
    /// line handed to `Wrap`, so at 150 columns it broke as "F4 /" then
    /// "forwards", and "F9 new /" then "session". The help screen taught two
    /// bindings that do not exist.
    #[test]
    fn no_binding_is_split_across_rows() {
        for width in [60, 72, 90, 120, 150] {
            let s = screen(width, 60, 0);
            for (key, label) in FUNCTION_KEYS {
                let want = format!("{} {}", key, label);
                assert!(
                    s.contains(&want),
                    "at {width} cols `{want}` is not on one row:\n{s}"
                );
            }
            // The specific broken renderings, named so a regression is obvious.
            for split in ["F4 /", "F9 new /", "/ forwards", "/ session"] {
                assert!(
                    !s.contains(split),
                    "at {width} cols a binding broke as `{split}`:\n{s}"
                );
            }
        }
    }

    /// The grid must survive a terminal narrower than one cell.
    #[test]
    fn the_key_grid_degrades_to_one_column_rather_than_dividing_by_zero() {
        let rows = key_grid(
            &[("F10", "command menu")],
            4,
            Style::default(),
            Style::default(),
        );
        assert_eq!(rows.len(), 1);
    }

    /// On a terminal too short for the whole reference, the tail must still be
    /// reachable — otherwise those keys do not exist as far as the user is
    /// concerned, and nothing on screen says so.
    #[test]
    fn the_end_of_the_reference_is_reachable_on_a_short_terminal() {
        let top = screen(80, 20, 0);
        assert!(top.contains("Toggle this help menu"), "help did not render");
        assert!(
            !top.contains("Press ? or Esc to close"),
            "this terminal is too tall to exercise scrolling"
        );

        let bottom = screen(80, 20, 200);
        assert!(
            bottom.contains("Press ? or Esc to close"),
            "the last line never comes into view:\n{bottom}"
        );
    }

    /// And the overlay says that there is more, rather than clipping silently.
    #[test]
    fn a_clipped_reference_shows_its_range() {
        let s = screen(80, 20, 0);
        assert!(
            s.contains(" of "),
            "no range indicator on a clipped help:\n{s}"
        );
    }

    /// Scrolling past the end must stop at the end, not run off into blank.
    #[test]
    fn scrolling_past_the_end_clamps() {
        assert_eq!(screen(80, 20, 200), screen(80, 20, 10_000));
    }

    #[test]
    fn a_tall_terminal_needs_no_scrolling_and_says_so() {
        let s = screen(90, 90, 0);
        assert!(s.contains("Press ? or Esc to close"));
        assert!(
            !s.contains(" of "),
            "range shown when nothing is clipped:\n{s}"
        );
    }
}

/// How many rows these lines occupy once wrapped to `width`.
///
/// `Paragraph::line_count` is unstable, and the naive `lines.len()` is wrong
/// the moment one line is longer than the popup — which is how the last
/// section of this reference became unreachable.
fn wrapped_height(lines: &[Line<'_>], width: u16) -> u16 {
    if width == 0 {
        return lines.len() as u16;
    }
    lines
        .iter()
        .map(|l| {
            let w: usize = l.spans.iter().map(|s| s.content.chars().count()).sum();
            // A blank line still occupies one row.
            (w.max(1) as u16).div_ceil(width)
        })
        .sum()
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    #[test]
    fn a_line_longer_than_the_width_counts_as_two_rows() {
        let lines = vec![Line::raw("x".repeat(30))];
        assert_eq!(wrapped_height(&lines, 20), 2);
        assert_eq!(wrapped_height(&lines, 30), 1);
    }

    #[test]
    fn a_blank_line_still_takes_a_row() {
        assert_eq!(wrapped_height(&[Line::raw("")], 20), 1);
    }
}
