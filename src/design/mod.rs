//! The ESSH 2.0 design system, transcribed from the handoff.
//!
//! Every constant here comes from `source/ESSH 2.0.html`. The handoff calls
//! itself high-fidelity — "colors, type scale, spacing, density and copy are
//! final" — so this module is the single place those values live, and the
//! screens read from it rather than inventing their own.
//!
//! ## What translates, and what does not
//!
//! The design was drawn for a GPU renderer with sub-cell primitives. This is
//! a terminal, so two things are approximated deliberately rather than faked:
//!
//! * **Vector sparklines become braille.** The handoff's own graph-primitives
//!   section carries the braille invariants forward "where braille is used",
//!   which is here. A braille cell resolves 2×4 dots, so a one-row graph has
//!   four vertical levels — the invariants below exist because of that.
//! * **The 3px peer ribbon has no equivalent.** A terminal has no sub-cell
//!   row, and spending a whole row on it would violate the chrome rule it was
//!   invented to satisfy. It is not drawn, rather than drawn wrongly.
//!
//! Everything else — palette, ramps, box idiom, density, copy — is exact.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

// ── Palette: the *Watch 2.0 family ───────────────────────────────────────
// Replaces v1's Tokyo-Night-ish theme.

mod panel;
// `panel` and `Bind` are the underlying API — used by the panel tests and by
// callers that hold a `Buffer` rather than a `Frame`.
#[allow(unused_imports)]
pub use panel::{pane, panel, Bind, PanelOpts};

pub const BG: Color = Color::Rgb(0x0c, 0x14, 0x18);
pub const FG: Color = Color::Rgb(0xc8, 0xd4, 0xd9);
pub const DIM: Color = Color::Rgb(0x6d, 0x81, 0x89);
pub const FAINT: Color = Color::Rgb(0x42, 0x55, 0x5d);
pub const RULE: Color = Color::Rgb(0x1c, 0x28, 0x2e);

pub const GREEN: Color = Color::Rgb(0x5c, 0xd9, 0x89);
pub const CYAN: Color = Color::Rgb(0x5f, 0xdc, 0xff);
pub const AMBER: Color = Color::Rgb(0xf0, 0xc0, 0x60);
pub const RED: Color = Color::Rgb(0xff, 0x78, 0x78);
pub const VIOLET: Color = Color::Rgb(0xb8, 0xa8, 0xe8);

/// White, for the values the eye should land on first (`.nm`, headline
/// numerals). Distinct from `FG`, which is body text.
pub const WHITE: Color = Color::Rgb(0xff, 0xff, 0xff);

/// The dot for a host with no signal: never probed, or offline.
///
/// Deliberately not grey text — it reads as "no signal", not as "bad". It was
/// `#33454d`, which measured 1.86:1 against the panel ground and was
/// therefore quiet to the point of absent, on the one glyph whose whole job
/// is to be noticed as *missing*. `#556a74` is 3.28:1: still the quietest
/// thing on the row, and now visible.
pub const NEVER_DOT: Color = Color::Rgb(0x55, 0x6a, 0x74);

/// Selection tint for the current row, `rgba(95,220,255,.05)` composited on
/// the background. Terminals have no alpha, so it is pre-mixed.
pub const ROW_SELECTED_BG: Color = Color::Rgb(0x10, 0x1a, 0x1f);

/// The green used inside meters and per-row sparklines — a shade below the
/// headline `GREEN` so a table of them does not glow.
pub const GREEN_MUTED: Color = Color::Rgb(0x3b, 0xb6, 0x73);

/// The title strip behind a panel's name, and the command palette's card.
/// One step off `BG`: enough for a border to read against, not enough to
/// become a second background. SPEC §5.2 collapses the reference's six
/// near-black grounds into three, and this is the middle one.
pub const PANEL_ALT: Color = Color::Rgb(0x10, 0x1a, 0x1f);

/// Scrollback and the command input. Below `BG`, so a shell reads as a hole
/// in the UI rather than as another panel.
pub const TERMINAL_BG: Color = Color::Rgb(0x08, 0x0f, 0x13);

/// The empty cells of a meter.
///
/// Has to read as a track. At `#16232a` on this panel ground it did not, so a
/// disk at 2% drew nothing at all and the column looked broken rather than
/// empty. A track you cannot see is the same defect as a meter you cannot
/// see, one value along.
pub const METER_TRACK: Color = Color::Rgb(0x24, 0x33, 0x3c);

/// The border of the focused pane. SPEC §4.1 hangs the quick-connect digits
/// on panel focus, so focus has to be visible and cannot be drawn from
/// `border`.
pub const BORDER_FOCUS: Color = Color::Rgb(0x2f, 0x55, 0x60);

/// Divergence ramp. Consensus is faint; the further from consensus, the
/// hotter. **Red means "you are alone"** — never "this number is large".
pub const RAMP_DIV: [Color; 5] = [
    Color::Rgb(0x2c, 0x3a, 0x42),
    Color::Rgb(0x4a, 0x5a, 0x60),
    Color::Rgb(0x8a, 0x8f, 0x6a),
    Color::Rgb(0xf0, 0xc0, 0x60),
    Color::Rgb(0xff, 0x78, 0x78),
];

/// Magnitude ramp, cool → bright. **High means busy, not bad.**
///
/// v1 coloured CPU and memory `<50% green / <80% yellow / >80% red`, which
/// cries wolf on every compile. A busy machine is a working machine.
pub const RAMP_OK: [Color; 5] = [
    Color::Rgb(0x13, 0x4f, 0x42),
    Color::Rgb(0x1f, 0x7d, 0x58),
    Color::Rgb(0x3b, 0xb6, 0x73),
    Color::Rgb(0x5c, 0xd9, 0x89),
    Color::Rgb(0xa6, 0xf2, 0xc0),
];

/// Utilisation ramp, green to amber to red, for a meter whose high end is
/// bad. SPEC §5.3.
///
/// Distinct from `RAMP_OK`, which is magnitude and never reaches red. This
/// one is read by *position along the scale*, so the amber stop sits where
/// 60% of the meter's width is, not where a value happens to fall.
pub const RAMP_UTIL: [Color; 5] = [
    Color::Rgb(0x35, 0xd0, 0x7f),
    Color::Rgb(0x8f, 0xcc, 0x66),
    Color::Rgb(0xe3, 0xbf, 0x4d),
    Color::Rgb(0xe8, 0x8f, 0x4f),
    Color::Rgb(0xef, 0x5f, 0x52),
];

/// Network ramp, magnitude, secondary.
pub const RAMP_NET: [Color; 5] = [
    Color::Rgb(0x10, 0x3f, 0x52),
    Color::Rgb(0x1c, 0x6f, 0x8c),
    Color::Rgb(0x3a, 0xa9, 0xc9),
    Color::Rgb(0x5f, 0xdc, 0xff),
    Color::Rgb(0xb6, 0xed, 0xff),
];

fn channels(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn mix(a: Color, b: Color, f: f64) -> Color {
    let (ar, ag, ab) = channels(a);
    let (br, bg, bb) = channels(b);
    let lerp = |x: u8, y: u8| -> u8 { (x as f64 + (y as f64 - x as f64) * f).round() as u8 };
    Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

/// Sample a ramp at `f` in 0.0–1.0, interpolating between stops.
pub fn ramp_at(ramp: &[Color], f: f64) -> Color {
    if ramp.is_empty() {
        return FG;
    }
    let t = f.clamp(0.0, 1.0) * (ramp.len() - 1) as f64;
    let i = t.floor() as usize;
    if i >= ramp.len() - 1 {
        ramp[ramp.len() - 1]
    } else {
        mix(ramp[i], ramp[i + 1], t - i as f64)
    }
}

// ── Meters ───────────────────────────────────────────────────────────────

/// A filled meter, matching `.mtr` — a solid bar on a faint track.
///
/// Uses the lower-block glyph so the bar reads as a bar rather than as text.
pub fn meter(fraction: f64, width: usize) -> (String, String) {
    let filled = ((fraction.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let filled = filled.min(width);
    ("█".repeat(filled), "░".repeat(width - filled))
}

/// A meter whose cells are coloured by **position along the scale**, not by
/// the value. SPEC §5.3.
///
/// This is the btop device the redesign is built on, and the distinction is
/// the whole point of it. Colour the run by value and a meter at 40% and one
/// at 95% are the same colour at their left edge, so the eye has to read the
/// length to learn anything. Colour by position and the amber cells sit at
/// 60% of the width on every row in the table, so a meter that reaches them
/// is visibly past the same mark as its neighbours.
///
/// Returns one span per filled cell, then one span for the track. Callers
/// push them straight into a `Line`.
///
/// Under a theme that defers to the terminal there is no gradient to sample:
/// a five-stop RGB ramp has no 16-colour form. The run collapses to a single
/// themed colour chosen by threshold, which is `design::meter`'s behaviour
/// with the theme's own slots.
pub fn meter_ramped(
    fraction: f64,
    width: usize,
    ramp: &[Color; 5],
    t: &crate::theme::Theme,
) -> Vec<Span<'static>> {
    let filled = ((fraction.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut out = Vec::with_capacity(filled + 1);

    if t.defers_to_terminal() {
        let flat = match fraction {
            f if f >= 0.85 => t.status_error,
            f if f >= 0.60 => t.status_warn,
            _ => t.status_good,
        };
        out.push(Span::styled("█".repeat(filled), Style::default().fg(flat)));
    } else {
        for cell in 0..filled {
            // Position of this cell's centre along the full scale. A
            // one-cell meter samples the middle of the ramp rather than its
            // cold end, which is what `cell / width` would give.
            let at = (cell as f64 + 0.5) / width.max(1) as f64;
            out.push(Span::styled("█", Style::default().fg(ramp_at(ramp, at))));
        }
    }

    if filled < width {
        out.push(Span::styled(
            "░".repeat(width - filled),
            Style::default().fg(t.meter_track),
        ));
    }
    out
}

/// A meter in one flat colour, on the themed track. Memory and disk, which
/// SPEC §5.3 keeps flat so that CPU's ramp is the only gradient in the row.
pub fn meter_flat(
    fraction: f64,
    width: usize,
    color: Color,
    t: &crate::theme::Theme,
) -> Vec<Span<'static>> {
    let (fill, track) = meter(fraction, width);
    let mut out = Vec::with_capacity(2);
    if !fill.is_empty() {
        out.push(Span::styled(fill, Style::default().fg(color)));
    }
    if !track.is_empty() {
        out.push(Span::styled(track, Style::default().fg(t.meter_track)));
    }
    out
}

// ── Cells ────────────────────────────────────────────────────────────────

/// Display width, in terminal cells.
pub fn width_of(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// Truncate to `width` cells, marking the cut.
///
/// The ellipsis is inside the budget, not added to it. A "truncation" that
/// overruns its column by one cell is how a table starts pushing everything
/// to its right off the screen.
pub fn ellipsise(text: &str, width: usize) -> String {
    if width_of(text) <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".repeat(width);
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// One left-aligned cell of exactly `width` cells.
pub fn fit_left(text: &str, width: usize, style: Style) -> Vec<Span<'static>> {
    let text = ellipsise(text, width);
    let pad = width.saturating_sub(width_of(&text));
    vec![Span::styled(text, style), Span::raw(" ".repeat(pad))]
}

/// One right-aligned cell of exactly `width` cells. Numbers go here.
pub fn fit_right(text: &str, width: usize, style: Style) -> Vec<Span<'static>> {
    let text = ellipsise(text, width);
    let pad = width.saturating_sub(width_of(&text));
    vec![Span::raw(" ".repeat(pad)), Span::styled(text, style)]
}

/// Pad a run of spans out to `width`, so a cell built from several styles
/// still occupies its column exactly.
pub fn pad_to(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let used: usize = spans.iter().map(|s| width_of(&s.content)).sum();
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

// ── Sparklines ───────────────────────────────────────────────────────────

const BRAILLE_DOTS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// A one-row braille sparkline of `width` cells covering `values`.
///
/// The invariants the handoff carries forward, and why each is here:
///
/// * **Levels are absolute, never set-relative.** `ceiling` is supplied by the
///   caller. Scaling to the series' own maximum makes every graph touch the
///   top, which throws the magnitude channel away.
/// * **Colour by value on a single-row graph.** Height is constant across one
///   row, so sampling a ramp by cell height would encode nothing.
/// * A cell resolves four vertical levels; a series whose whole deviation
///   lands in one rounding bucket renders as a single repeated glyph, which is
///   why callers pass a ceiling derived from the measured peak.
pub fn sparkline(values: &[u64], width: usize, ceiling: u64) -> String {
    if width == 0 {
        return String::new();
    }
    if values.is_empty() {
        return " ".repeat(width);
    }
    let ceiling = ceiling.max(1) as f64;

    // Two horizontal dots per cell, so sample 2× the cell count.
    let samples = width * 2;
    let mut out = String::with_capacity(width * 3);
    let mut cell: u8 = 0;

    for s in 0..samples {
        // Map sample position back onto the series, newest at the right.
        let idx = if samples == 1 {
            values.len() - 1
        } else {
            (s * (values.len().saturating_sub(1))) / (samples - 1)
        };
        let v = values[idx.min(values.len() - 1)] as f64;
        let level = ((v / ceiling).clamp(0.0, 1.0) * 4.0).round().max(1.0) as usize;

        let col = s % 2;
        for row in 0..level.min(4) {
            // Braille row 0 is the bottom of the drawn column.
            cell |= BRAILLE_DOTS[col][3 - row];
        }
        if col == 1 {
            out.push(char::from_u32(0x2800 + cell as u32).unwrap_or(' '));
            cell = 0;
        }
    }
    if samples % 2 == 1 {
        out.push(char::from_u32(0x2800 + cell as u32).unwrap_or(' '));
    }
    out
}

/// The ceiling for a series, on a ladder with rungs no more than 25% apart.
///
/// Derived from the measured peak rather than fixed, so a quiet series still
/// uses the full four levels instead of flatlining along the bottom.
/// A ceiling for an RTT graph, never below `RTT_FLOOR_MS`.
///
/// `axis_ceiling` scales to the series' own peak, which is right for a graph
/// whose magnitude is the point and wrong for latency. A fleet on loopback
/// answers in 3ms every time, and a self-scaled graph draws that as a solid
/// wall of full-height cells: the healthiest possible reading rendered as the
/// worst possible picture. An absolute floor keeps a healthy host flat and
/// low, and the ceiling still rises when something is genuinely slow.
pub const RTT_FLOOR_MS: u64 = 20;

pub fn rtt_ceiling(values: &[u64]) -> u64 {
    axis_ceiling(values).max(RTT_FLOOR_MS)
}

pub fn axis_ceiling(values: &[u64]) -> u64 {
    let peak = values.iter().copied().max().unwrap_or(1).max(1);
    let mut rung = 1u64;
    while rung < peak {
        // 1, 2, 3, 4, 5, 7, 10, 13, 17, 21, … — each ≤25% above the last
        // once past the small integers.
        rung = (rung as f64 * 1.25).ceil() as u64;
    }
    rung
}

// ── Widgets ──────────────────────────────────────────────────────────────

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::Frame;

/// A column header cell: faint, uppercase. The `DIVERGE` column is cyan
/// (`th.s` in the handoff) because it is the one the eye should find.
pub fn header_cell(label: &str, emphasised: bool, t: &crate::theme::Theme) -> Span<'static> {
    Span::styled(
        label.to_uppercase(),
        Style::default().fg(if emphasised { t.brand } else { t.border }),
    )
}

/// The status dot: a filled circle, coloured by state. Never-probed uses a
/// colour that reads as "no signal" rather than as a bad reading.
pub fn dot(color: Color) -> Span<'static> {
    Span::styled("●", Style::default().fg(color))
}

/// A footer of `key label` pairs, cyan key and faint label, matching `.foot`.
pub fn footer_line(pairs: &[(&str, &str)], t: &crate::theme::Theme) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(t.key_hint).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            label.to_string(),
            Style::default().fg(t.border),
        ));
    }
    Line::from(spans)
}

/// A headline numeral with its unit and a sub-line, per the handoff's `big()`.
///
/// The sub-line is where the peer median lives: *"every headline metric
/// carries its peer median, so 23% becomes 23% against a fleet median of
/// 31%"*. A number alone is not a finding.
#[allow(dead_code)] // composed inline by the monitor's headline_box
pub fn headline<'a>(
    value: &'a str,
    unit: &'a str,
    sub: &'a str,
    t: &crate::theme::Theme,
) -> Vec<Line<'a>> {
    vec![
        Line::from(vec![
            Span::styled(
                value,
                Style::default()
                    .fg(t.text_emphasis)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(unit, Style::default().fg(t.text_secondary)),
        ]),
        Line::from(Span::styled(sub, Style::default().fg(t.border))),
    ]
}

/// An honest empty state: italic, faint, in words. Never a zero, never a
/// dash in a laid-out column, never a stub bar.
pub fn none_line(text: &str, t: &crate::theme::Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(t.border).add_modifier(Modifier::ITALIC),
    ))
}

/// Paint a region with the design background, so the app owns its canvas
/// rather than inheriting whatever the host terminal happens to use.
/// Fill a region with the theme's own ground.
///
/// This used to hardcode the 2.0 palette's `BG`/`FG`, which is why a light
/// theme could not work: its near-black text landed on a painted dark panel.
/// A theme that wants the terminal's background leaves `bg` as `Color::Reset`,
/// which paints nothing.
pub fn paint_bg(f: &mut Frame, area: Rect, theme: &crate::theme::Theme) {
    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.text_primary)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_the_handoff_exactly() {
        // These are quoted in the handoff's colour section; a drift here
        // means the screens no longer match the design they cite.
        assert_eq!(BG, Color::Rgb(0x0c, 0x14, 0x18));
        assert_eq!(FG, Color::Rgb(0xc8, 0xd4, 0xd9));
        assert_eq!(DIM, Color::Rgb(0x6d, 0x81, 0x89));
        assert_eq!(FAINT, Color::Rgb(0x42, 0x55, 0x5d));
        assert_eq!(RULE, Color::Rgb(0x1c, 0x28, 0x2e));
        assert_eq!(GREEN, Color::Rgb(0x5c, 0xd9, 0x89));
        assert_eq!(CYAN, Color::Rgb(0x5f, 0xdc, 0xff));
        assert_eq!(AMBER, Color::Rgb(0xf0, 0xc0, 0x60));
        assert_eq!(RED, Color::Rgb(0xff, 0x78, 0x78));
        assert_eq!(VIOLET, Color::Rgb(0xb8, 0xa8, 0xe8));
    }

    #[test]
    fn meter_fills_proportionally_and_never_overflows() {
        let (f, e) = meter(0.42, 10);
        assert_eq!(f.chars().count(), 4);
        assert_eq!(f.chars().count() + e.chars().count(), 10);

        let (f, e) = meter(1.5, 10);
        assert_eq!(f.chars().count(), 10);
        assert_eq!(e.chars().count(), 0);

        let (f, _) = meter(-1.0, 10);
        assert_eq!(f.chars().count(), 0);
    }

    #[test]
    fn a_sparkline_is_exactly_the_width_asked_for() {
        for w in [1usize, 7, 26, 74] {
            let s = sparkline(&[1, 5, 3, 9, 2], w, 10);
            assert_eq!(s.chars().count(), w, "width {}", w);
        }
    }

    #[test]
    fn an_empty_series_draws_nothing_rather_than_a_flat_line() {
        // Drawing a graph for data you do not have is worse than drawing
        // nothing — the handoff's rule, and the v1 defect it names.
        let s = sparkline(&[], 10, 100);
        assert_eq!(s, "          ");
        assert!(!s.contains('⣿'));
    }

    #[test]
    fn levels_are_absolute_so_a_quiet_series_stays_low() {
        // Set-relative scaling would make both of these look identical.
        let quiet = sparkline(&[1, 2, 1, 2], 8, 100);
        let busy = sparkline(&[90, 95, 92, 99], 8, 100);
        assert_ne!(quiet, busy);
        // The quiet one must use the lowest dots only.
        assert!(
            quiet.chars().all(|c| {
                let bits = c as u32 - 0x2800;
                // Only the bottom row of each column: 0x40 and 0x80.
                bits & !(0x40 | 0x80) == 0
            }),
            "quiet series climbed: {}",
            quiet
        );
    }

    #[test]
    fn axis_ceiling_climbs_in_rungs_no_more_than_25_percent_apart() {
        assert!(axis_ceiling(&[1]) >= 1);
        for peak in [3u64, 17, 42, 100, 999] {
            let c = axis_ceiling(&[peak]);
            assert!(c >= peak, "ceiling {} below peak {}", c, peak);
            assert!(
                c as f64 <= peak as f64 * 1.25 + 1.0,
                "ceiling {} too far above peak {}",
                c,
                peak
            );
        }
    }

    #[test]
    fn a_series_at_its_ceiling_reaches_the_top_row() {
        let s = sparkline(&[10, 10, 10], 4, 10);
        // Full column = all four dots in both sub-columns.
        assert!(s.chars().all(|c| c == '⣿'), "got {}", s);
    }

    // ── Ramped meters (SPEC §5.3) ────────────────────────────────────────

    fn colours(spans: &[ratatui::text::Span]) -> Vec<Color> {
        spans
            .iter()
            .flat_map(|s| std::iter::repeat_n(s.style.fg.expect("fg"), s.content.chars().count()))
            .collect()
    }

    /// The distinction the whole device rests on.
    ///
    /// A meter coloured by value paints every one of its cells the same, so a
    /// column of them tells you nothing until you measure their lengths. A
    /// meter coloured by position puts the amber cells at the same column on
    /// every row, and a bar that reaches them has visibly passed the same
    /// mark as its neighbours.
    #[test]
    fn a_ramped_meter_colours_by_position_not_by_value() {
        let t = crate::theme::dark();
        let full = meter_ramped(1.0, 10, &RAMP_UTIL, &t);
        let cs = colours(&full);
        assert_eq!(cs.len(), 10);
        assert_ne!(cs[0], cs[9], "a full meter is a gradient, not a block");

        // The same cell is the same colour whatever the value, because its
        // colour comes from where it sits on the scale.
        let half = meter_ramped(0.5, 10, &RAMP_UTIL, &t);
        let hs = colours(&half);
        assert_eq!(hs.len(), 10);
        for i in 0..5 {
            assert_eq!(hs[i], cs[i], "cell {i} changed colour with the value");
        }
    }

    #[test]
    fn a_ramped_meter_tracks_its_empty_cells() {
        let t = crate::theme::dark();
        let spans = meter_ramped(0.3, 10, &RAMP_UTIL, &t);
        let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered.chars().count(), 10);
        assert_eq!(rendered.matches('█').count(), 3);
        assert_eq!(rendered.matches('░').count(), 7);
        assert_eq!(colours(&spans)[9], t.meter_track);
    }

    /// The `terminal` theme pins no RGB. A five-stop gradient has no
    /// 16-colour form, so the run collapses to one themed slot rather than
    /// leaking the fixed values the theme exists to avoid.
    #[test]
    fn the_terminal_theme_gets_a_flat_meter_and_no_rgb() {
        let t = crate::theme::terminal();
        for (fraction, expected) in [
            (0.2, t.status_good),
            (0.7, t.status_warn),
            (0.95, t.status_error),
        ] {
            let spans = meter_ramped(fraction, 12, &RAMP_UTIL, &t);
            let filled: Vec<Color> = spans
                .iter()
                .filter(|s| s.content.contains('█'))
                .map(|s| s.style.fg.expect("fg"))
                .collect();
            assert_eq!(filled, vec![expected], "fraction {fraction}");
            for span in &spans {
                assert!(
                    !matches!(span.style.fg, Some(Color::Rgb(..))),
                    "terminal theme leaked an RGB value"
                );
            }
        }
    }

    /// A one-cell meter samples the middle of the ramp. `cell / width` would
    /// give it the cold end, so every narrow meter in a table would read as
    /// healthy no matter what it measured.
    #[test]
    fn a_one_cell_meter_is_not_stuck_at_the_cold_end() {
        let t = crate::theme::dark();
        let one = meter_ramped(1.0, 1, &RAMP_UTIL, &t);
        assert_eq!(colours(&one)[0], ramp_at(&RAMP_UTIL, 0.5));
    }

    #[test]
    fn a_flat_meter_is_one_colour_on_the_themed_track() {
        let t = crate::theme::dark();
        let spans = meter_flat(0.5, 8, t.status_info, &t);
        let cs = colours(&spans);
        assert_eq!(cs.len(), 8);
        assert!(cs[..4].iter().all(|c| *c == t.status_info));
        assert!(cs[4..].iter().all(|c| *c == t.meter_track));
    }

    #[test]
    fn an_empty_and_a_full_meter_still_measure_their_width() {
        let t = crate::theme::dark();
        for f in [0.0, 1.0] {
            for w in [1usize, 7, 8, 20] {
                let ramped: String = meter_ramped(f, w, &RAMP_UTIL, &t)
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                let flat: String = meter_flat(f, w, t.status_good, &t)
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                assert_eq!(ramped.chars().count(), w);
                assert_eq!(flat.chars().count(), w);
            }
        }
    }
}
