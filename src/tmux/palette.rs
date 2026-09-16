//! One theme shared by the workspace UI and its tmux chrome.
use crate::theme::{self, Theme};
use ratatui::style::Color;
use std::sync::OnceLock;

static THEME: OnceLock<Theme> = OnceLock::new();

pub fn init(name: &str) {
    let _ = THEME.set(theme::by_name(name));
}

pub fn current() -> &'static Theme {
    THEME.get_or_init(theme::terminal)
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

// Translate the chrome's design roles, never shell content or host arguments.
pub fn style(template: &str) -> String {
    style_for(template, current())
}

fn style_for(template: &str, theme: &Theme) -> String {
    let mut result = template.to_owned();
    for (token, color) in [
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
    fn terminal_chrome_uses_default_background_and_ansi_palette() {
        assert_eq!(
            style_for("bg=$footer,fg=$brand", &theme::terminal()),
            "bg=default,fg=colour6"
        );
        assert_eq!(tmux_color(Color::Reset), "default");
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
