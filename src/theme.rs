use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::config::Config;

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

mod latte {
    use ratatui::style::Color;

    pub const MAUVE: Color = Color::from_u32(0x8839ef);
    pub const RED: Color = Color::from_u32(0xd20f39);
    pub const PEACH: Color = Color::from_u32(0xfe640b);
    pub const YELLOW: Color = Color::from_u32(0xdf8e1d);
    pub const GREEN: Color = Color::from_u32(0x40a02b);
    pub const SKY: Color = Color::from_u32(0x04a5e5);
    pub const BLUE: Color = Color::from_u32(0x1e66f5);
    pub const TEXT: Color = Color::from_u32(0x4c4f69);
    pub const SUBTEXT0: Color = Color::from_u32(0x6c6f85);
    pub const SURFACE1: Color = Color::from_u32(0xbcc0cc);
    pub const SURFACE0: Color = Color::from_u32(0xccd0da);
    pub const BASE: Color = Color::from_u32(0xeff1f5);
    pub const MANTLE: Color = Color::from_u32(0xe6e9ef);
}

/// Built-in palette that `colors` overrides are applied to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Mocha,
    Light,
    /// The terminal's own colors and background.
    Terminal,
}

macro_rules! color_overrides {
    ($($field:ident),* $(,)?) => {
        /// Per-color overrides on top of a [`ThemeName`] preset.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct ColorOverrides {
            $(
                #[serde(skip_serializing_if = "Option::is_none")]
                pub $field: Option<Color>,
            )*
        }

        impl ColorOverrides {
            /// Configs written before presets hold the whole Mocha palette; a color equal to
            /// Mocha's is not an override unless the Mocha preset is chosen.
            fn apply(&self, theme: &mut Theme, name: ThemeName) {
                let mocha = Theme::default();
                $(
                    if let Some(color) = self.$field
                        && (name == ThemeName::Mocha || color != mocha.$field)
                    {
                        theme.$field = color;
                    }
                )*
            }
        }
    };
}

color_overrides!(
    background,
    foreground,
    border,
    border_focused,
    selection_bg,
    selection_fg,
    title,
    toolbar_bg,
    toolbar_key,
    pr_approved,
    pr_waiting,
    pr_rejected,
    build_succeeded,
    build_failed,
    build_running,
    workitem_bug,
    workitem_story,
    workitem_task,
    comment_author,
    muted,
    error,
);

impl Theme {
    pub fn from_config(config: &Config) -> Self {
        let mut theme = match config.theme {
            ThemeName::Mocha => Self::default(),
            ThemeName::Light => Self::light(),
            ThemeName::Terminal => Self::terminal(),
        };
        config.colors.apply(&mut theme, config.theme);
        theme
    }

    fn light() -> Self {
        Self {
            background: latte::BASE,
            foreground: latte::TEXT,
            border: latte::SURFACE1,
            border_focused: latte::BLUE,
            selection_bg: latte::SURFACE0,
            selection_fg: latte::TEXT,
            title: latte::MAUVE,
            toolbar_bg: latte::MANTLE,
            toolbar_key: latte::YELLOW,
            pr_approved: latte::GREEN,
            pr_waiting: latte::YELLOW,
            pr_rejected: latte::RED,
            build_succeeded: latte::GREEN,
            build_failed: latte::RED,
            build_running: latte::SKY,
            workitem_bug: latte::RED,
            workitem_story: latte::BLUE,
            workitem_task: latte::YELLOW,
            comment_author: latte::PEACH,
            muted: latte::SUBTEXT0,
            error: latte::RED,
        }
    }

    fn terminal() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Reset,
            border: Color::DarkGray,
            border_focused: Color::Blue,
            selection_bg: Color::DarkGray,
            selection_fg: Color::Reset,
            title: Color::Magenta,
            toolbar_bg: Color::Reset,
            toolbar_key: Color::Yellow,
            pr_approved: Color::Green,
            pr_waiting: Color::Yellow,
            pr_rejected: Color::Red,
            build_succeeded: Color::Green,
            build_failed: Color::Red,
            build_running: Color::Cyan,
            workitem_bug: Color::Red,
            workitem_story: Color::Blue,
            workitem_task: Color::Yellow,
            comment_author: Color::Yellow,
            muted: Color::DarkGray,
            error: Color::Red,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_colors_keep_mocha_defaults() {
        let theme: Theme = serde_json::from_str(r##"{"background":"#000000"}"##).unwrap();

        assert_eq!(theme.background, Color::Rgb(0, 0, 0));
        assert_eq!(theme.foreground, mocha::TEXT);
        assert_eq!(theme.border_focused, mocha::BLUE);
    }

    #[test]
    fn default_theme_is_mocha() {
        let theme = Theme::default();

        assert_eq!(theme.background, Color::from_u32(0x1e1e2e));
        assert_eq!(theme.error, mocha::RED);
    }

    fn config(json: &str) -> Config {
        let json = format!(r#"{{"organization":"o","project":"p","team":"t",{json}}}"#);
        serde_json::from_str(&json).unwrap()
    }

    fn all(theme: &Theme) -> Vec<Color> {
        let json = serde_json::to_value(theme).unwrap();
        json.as_object()
            .unwrap()
            .values()
            .map(|value| serde_json::from_value(value.clone()).unwrap())
            .collect()
    }

    #[test]
    fn presets_fill_every_color() {
        for name in ["mocha", "light", "terminal"] {
            let theme = Theme::from_config(&config(&format!(r#""theme":"{name}""#)));

            assert_eq!(all(&theme).len(), 21, "{name}");
        }
    }

    #[test]
    fn no_theme_is_mocha() {
        let theme = Theme::from_config(&config(r#""notifications":false"#));

        assert_eq!(theme.background, mocha::BASE);
    }

    #[test]
    fn light_and_terminal_presets() {
        let light = Theme::from_config(&config(r#""theme":"light""#));
        let terminal = Theme::from_config(&config(r#""theme":"terminal""#));

        assert_eq!(light.background, Color::Rgb(0xef, 0xf1, 0xf5));
        assert_eq!(light.foreground, latte::TEXT);
        assert_eq!(terminal.background, Color::Reset);
        assert_eq!(terminal.foreground, Color::Reset);
        assert_eq!(terminal.toolbar_bg, Color::Reset);
        assert_eq!(terminal.pr_approved, Color::Green);
    }

    #[test]
    fn overrides_apply_on_top_of_the_preset() {
        let theme = Theme::from_config(&config(
            r##""theme":"light","colors":{"background":"#000000"}"##,
        ));

        assert_eq!(theme.background, Color::Rgb(0, 0, 0));
        assert_eq!(theme.foreground, latte::TEXT);
    }

    #[test]
    fn legacy_full_mocha_palette_does_not_hide_the_preset() {
        let colors = serde_json::to_string(&Theme::default()).unwrap();

        let light = Theme::from_config(&config(&format!(r#""theme":"light","colors":{colors}"#)));
        let mocha = Theme::from_config(&config(&format!(r#""colors":{colors}"#)));

        assert_eq!(light.background, latte::BASE);
        assert_eq!(light.error, latte::RED);
        assert_eq!(mocha.background, mocha::BASE);
    }

    #[test]
    fn legacy_palette_keeps_a_changed_color() {
        let mut palette = serde_json::to_value(Theme::default()).unwrap();
        palette["background"] = "#000000".into();

        let theme = Theme::from_config(&config(&format!(r#""theme":"light","colors":{palette}"#)));

        assert_eq!(theme.background, Color::Rgb(0, 0, 0));
        assert_eq!(theme.foreground, latte::TEXT);
    }

    #[test]
    fn old_config_with_only_background_keeps_it() {
        let theme = Theme::from_config(&config(r##""colors":{"background":"#000000"}"##));

        assert_eq!(theme.background, Color::Rgb(0, 0, 0));
        assert_eq!(theme.foreground, mocha::TEXT);
    }

    #[test]
    fn colors_round_trip_as_hex() {
        let json = serde_json::to_value(Theme::default()).unwrap();

        assert_eq!(json["background"], "#1E1E2E");
        let theme: Theme = serde_json::from_value(json).unwrap();
        assert_eq!(theme.background, mocha::BASE);
    }
}
