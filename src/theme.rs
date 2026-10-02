use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub border: Color,
    pub border_focused: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub title: Color,
    pub toolbar_bg: Color,
    pub toolbar_key: Color,
    pub pr_approved: Color,
    pub pr_waiting: Color,
    pub pr_rejected: Color,
    pub build_succeeded: Color,
    pub build_failed: Color,
    pub build_running: Color,
    pub workitem_bug: Color,
    pub workitem_story: Color,
    pub workitem_task: Color,
    pub comment_author: Color,
    pub muted: Color,
    pub error: Color,
}

mod mocha {
    use ratatui::style::Color;

    pub const ROSEWATER: Color = Color::from_u32(0xf5e0dc);
    pub const MAUVE: Color = Color::from_u32(0xcba6f7);
    pub const RED: Color = Color::from_u32(0xf38ba8);
    pub const PEACH: Color = Color::from_u32(0xfab387);
    pub const YELLOW: Color = Color::from_u32(0xf9e2af);
    pub const GREEN: Color = Color::from_u32(0xa6e3a1);
    pub const SKY: Color = Color::from_u32(0x89dceb);
    pub const BLUE: Color = Color::from_u32(0x89b4fa);
    pub const TEXT: Color = Color::from_u32(0xcdd6f4);
    pub const OVERLAY0: Color = Color::from_u32(0x6c7086);
    pub const SURFACE1: Color = Color::from_u32(0x45475a);
    pub const SURFACE0: Color = Color::from_u32(0x313244);
    pub const BASE: Color = Color::from_u32(0x1e1e2e);
    pub const MANTLE: Color = Color::from_u32(0x181825);
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: mocha::BASE,
            foreground: mocha::TEXT,
            border: mocha::SURFACE1,
            border_focused: mocha::BLUE,
            selection_bg: mocha::SURFACE0,
            selection_fg: mocha::ROSEWATER,
            title: mocha::MAUVE,
            toolbar_bg: mocha::MANTLE,
            toolbar_key: mocha::YELLOW,
            pr_approved: mocha::GREEN,
            pr_waiting: mocha::YELLOW,
            pr_rejected: mocha::RED,
            build_succeeded: mocha::GREEN,
            build_failed: mocha::RED,
            build_running: mocha::SKY,
            workitem_bug: mocha::RED,
            workitem_story: mocha::BLUE,
            workitem_task: mocha::YELLOW,
            comment_author: mocha::PEACH,
            muted: mocha::OVERLAY0,
            error: mocha::RED,
        }
    }
}
