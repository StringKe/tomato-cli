use std::env;
use std::sync::OnceLock;
use std::time::Duration;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use terminal_colorsaurus::{theme_mode, QueryOptions, ThemeMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeId {
    #[default]
    Auto,
    Night,
    Day,
    Parchment,
    Forest,
    Contrast,
}

impl ThemeId {
    pub const ALL: [ThemeId; 6] = [ThemeId::Auto, ThemeId::Night, ThemeId::Day, ThemeId::Parchment, ThemeId::Forest, ThemeId::Contrast];

    pub fn name(self) -> String {
        match self {
            ThemeId::Auto => match terminal_is_dark() {
                Some(true) => "Auto（终端 深色）".into(),
                Some(false) => "Auto（终端 浅色）".into(),
                None => "Auto（跟随终端）".into(),
            },
            ThemeId::Night => "夜间".into(),
            ThemeId::Day => "日间".into(),
            ThemeId::Parchment => "羊皮纸".into(),
            ThemeId::Forest => "护眼绿".into(),
            ThemeId::Contrast => "高对比".into(),
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub inherit: bool,
    pub reversed: bool,
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub title: Color,
    pub border: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
}

pub fn palette(id: ThemeId) -> Palette {
    match id {
        ThemeId::Auto => auto_palette(),
        ThemeId::Night => Palette {
            inherit: false,
            reversed: false,
            bg: Color::Rgb(18, 18, 18),
            fg: Color::Rgb(220, 220, 220),
            dim: Color::Rgb(120, 120, 120),
            accent: Color::Rgb(232, 93, 93),
            title: Color::Rgb(255, 180, 162),
            border: Color::Rgb(70, 70, 70),
            highlight_bg: Color::Rgb(50, 36, 36),
            highlight_fg: Color::Rgb(255, 220, 210),
        },
        ThemeId::Day => Palette {
            inherit: false,
            reversed: false,
            bg: Color::Rgb(248, 248, 246),
            fg: Color::Rgb(32, 32, 32),
            dim: Color::Rgb(110, 110, 110),
            accent: Color::Rgb(180, 40, 40),
            title: Color::Rgb(140, 30, 30),
            border: Color::Rgb(180, 180, 180),
            highlight_bg: Color::Rgb(255, 220, 210),
            highlight_fg: Color::Rgb(40, 20, 20),
        },
        ThemeId::Parchment => Palette {
            inherit: false,
            reversed: false,
            bg: Color::Rgb(244, 228, 196),
            fg: Color::Rgb(62, 43, 31),
            dim: Color::Rgb(130, 100, 70),
            accent: Color::Rgb(140, 70, 30),
            title: Color::Rgb(110, 50, 20),
            border: Color::Rgb(180, 140, 90),
            highlight_bg: Color::Rgb(230, 190, 140),
            highlight_fg: Color::Rgb(50, 30, 10),
        },
        ThemeId::Forest => Palette {
            inherit: false,
            reversed: false,
            bg: Color::Rgb(18, 32, 24),
            fg: Color::Rgb(198, 220, 198),
            dim: Color::Rgb(100, 130, 100),
            accent: Color::Rgb(120, 190, 120),
            title: Color::Rgb(170, 220, 160),
            border: Color::Rgb(50, 80, 50),
            highlight_bg: Color::Rgb(36, 64, 42),
            highlight_fg: Color::Rgb(220, 255, 220),
        },
        ThemeId::Contrast => Palette {
            inherit: false,
            reversed: false,
            bg: Color::Black,
            fg: Color::White,
            dim: Color::Gray,
            accent: Color::Yellow,
            title: Color::Yellow,
            border: Color::White,
            highlight_bg: Color::White,
            highlight_fg: Color::Black,
        },
    }
}

fn auto_palette() -> Palette {
    let dark = terminal_is_dark().unwrap_or(true);
    Palette {
        inherit: true,
        reversed: true,
        bg: Color::Reset,
        fg: Color::Reset,
        dim: if dark { Color::Gray } else { Color::DarkGray },
        accent: if dark { Color::LightRed } else { Color::Red },
        title: Color::Reset,
        border: Color::Gray,
        highlight_bg: Color::Reset,
        highlight_fg: Color::Reset,
    }
}

static TERMINAL_DARK: OnceLock<Option<bool>> = OnceLock::new();

/// 进入 TUI 之前探测终端深浅色。先 OSC 10/11，再 `COLORFGBG`。
pub fn probe_terminal() {
    let _ = TERMINAL_DARK.get_or_init(detect_terminal);
}

pub fn terminal_is_dark() -> Option<bool> {
    if let Some(cached) = TERMINAL_DARK.get() {
        return *cached;
    }
    env::var("COLORFGBG").ok().as_deref().and_then(parse_colorfgbg)
}

fn detect_terminal() -> Option<bool> {
    let mut options = QueryOptions::default();
    options.timeout = Duration::from_millis(250);
    if let Ok(mode) = theme_mode(options) {
        return Some(mode == ThemeMode::Dark);
    }
    env::var("COLORFGBG").ok().as_deref().and_then(parse_colorfgbg)
}

/// `COLORFGBG` 形如 `15;0`（浅字深底）或 `0;15`（深字浅底）。
pub fn parse_colorfgbg(s: &str) -> Option<bool> {
    let bg = s.split([';', ':']).next_back()?.trim().parse::<i32>().ok()?;
    if (0..=6).contains(&bg) || bg == 8 {
        Some(true)
    } else if bg == 7 || (9..=15).contains(&bg) {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorfgbg_dark_bg() {
        assert_eq!(parse_colorfgbg("15;0"), Some(true));
        assert_eq!(parse_colorfgbg("7;0"), Some(true));
    }

    #[test]
    fn colorfgbg_light_bg() {
        assert_eq!(parse_colorfgbg("0;15"), Some(false));
        assert_eq!(parse_colorfgbg("0;7"), Some(false));
    }

    #[test]
    fn auto_is_default() {
        assert_eq!(ThemeId::default(), ThemeId::Auto);
    }
}
