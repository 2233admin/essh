//! One theme shared by the workspace UI, terminal defaults and tmux chrome.
use crate::theme::{self, Theme};
use ratatui::style::Color;
use std::sync::{OnceLock, RwLock};

static THEME: OnceLock<RwLock<Theme>> = OnceLock::new();

pub fn init(name: &str) {
    *THEME
        .get_or_init(|| RwLock::new(theme::terminal()))
        .write()
        .unwrap() = theme::by_name(name);
}

pub fn current() -> Theme {
    THEME
        .get_or_init(|| RwLock::new(theme::terminal()))
        .read()
        .unwrap()
        .clone()
}

fn tmux_color(color: Color) -> String {
    match color {
        Color::Reset => "default".into(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(n) => format!("colour{n}"),
        other => {
            let colors = [
                Color::Black,
                Color::Red,
                Color::Green,
                Color::Yellow,
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::Gray,
                Color::DarkGray,
                Color::LightRed,
                Color::LightGreen,
                Color::LightYellow,
                Color::LightBlue,
                Color::LightMagenta,
                Color::LightCyan,
                Color::White,
            ];
            format!("colour{}", colors.iter().position(|c| *c == other).unwrap())
        }
    }
}

// Translate style roles, never shell content or host arguments.
pub fn style(template: &str) -> String {
    style_for(template, &current())
}

fn style_for(template: &str, theme: &Theme) -> String {
    let mut result = template.to_owned();
    for (token, color) in [
        ("$terminal", theme.terminal_bg),
        ("$text", theme.text_primary),
        ("$footer", theme.footer_bg),
        ("$secondary", theme.text_secondary),
        ("$brand", theme.brand),
        ("$border", theme.border),
        ("$selection", theme.selection_bg),
        ("$good", theme.status_good),
        ("$error", theme.status_error),
    ] {
        result = result.replace(token, &tmux_color(color));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netwatch_has_a_persistent_name_and_the_design_palette() {
        let netwatch = theme::by_name("netwatch");
        assert_eq!(netwatch.name, "netwatch");
        assert_eq!(netwatch.bg, theme::dark().bg);
        assert_eq!(netwatch.brand, theme::dark().brand);
        assert_eq!(netwatch.status_good, theme::dark().status_good);
    }

    #[test]
    fn terminal_chrome_uses_default_background_and_ansi_palette() {
        assert_eq!(
            style_for("bg=$footer,fg=$brand", &theme::terminal()),
            "bg=default,fg=colour6"
        );
        assert_eq!(tmux_color(Color::Reset), "default");
    }

    #[test]
    fn terminal_defaults_follow_the_selected_palette() {
        for (name, expected) in [
            ("terminal", "bg=default,fg=default"),
            ("netwatch", "bg=#080f13,fg=#c8d4d9"),
            ("light", "bg=#ffffff,fg=#1e1e1e"),
        ] {
            assert_eq!(
                style_for("bg=$terminal,fg=$text", &theme::by_name(name)),
                expected
            );
        }
    }

    #[test]
    fn explicit_theme_changes_chrome_and_preserves_tmux_formats() {
        let input = "#[fg=$brand]#{window_name}#[default]";
        let expected = format!(
            "#[fg={}]#{{window_name}}#[default]",
            tmux_color(theme::light().brand)
        );
        assert_eq!(style_for(input, &theme::light()), expected);
        assert_ne!(
            style_for(input, &theme::light()),
            style_for(input, &theme::terminal())
        );
    }
}
