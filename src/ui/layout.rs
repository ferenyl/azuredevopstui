use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::{App, LEFT_PANELS, Panel};

const DEFAULT_LEFT_PERCENT: u16 = 25;
const MIN_LEFT_PERCENT: u16 = 15;
const MAX_LEFT_PERCENT: u16 = 60;
/// The list column is at least this wide, up to half the screen.
const MIN_LEFT_WIDTH: u16 = 32;
/// Below this many rows per left panel, only the focused one keeps its rows.
const MIN_PANEL_ROWS: u16 = 5;

/// Where each panel is drawn; panels left out are hidden.
pub struct Areas {
    pub panels: Vec<(Panel, Rect)>,
    pub detail: Option<Rect>,
}

pub fn layout(app: &App, main: Rect) -> Areas {
    if app.zoomed && app.focus == Panel::Detail {
        return Areas {
            panels: Vec::new(),
            detail: Some(main),
        };
    }
    let percent = app
        .config
        .as_ref()
        .map_or(DEFAULT_LEFT_PERCENT, |config| config.layout.left_percent)
        .clamp(MIN_LEFT_PERCENT, MAX_LEFT_PERCENT);
    let width = (u32::from(main.width) * u32::from(percent) / 100) as u16;
    let width = width.max(MIN_LEFT_WIDTH.min(main.width / 2));
    let [left, right] =
        Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).areas(main);

    let expanded = app.left_focus();
    let panels = if app.zoomed {
        vec![(expanded, left)]
    } else {
        let constraints = if left.height < LEFT_PANELS.len() as u16 * MIN_PANEL_ROWS {
            // Collapsed panels keep a title bar.
            LEFT_PANELS.map(|panel| {
                if panel == expanded {
                    Constraint::Min(0)
                } else {
                    Constraint::Length(1)
                }
            })
        } else {
            [Constraint::Ratio(1, LEFT_PANELS.len() as u32); LEFT_PANELS.len()]
        };
        LEFT_PANELS
            .into_iter()
            .zip(Layout::vertical(constraints).split(left).iter().copied())
            .collect()
    };
    Areas {
        panels,
        detail: Some(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::test_support::config;

    fn app() -> App {
        let mut app = App::new(Some(config()), None);
        app.screen = Screen::Main;
        app
    }

    fn main(width: u16, height: u16) -> Rect {
        // The toolbar takes the last row.
        Rect::new(0, 0, width, height - 1)
    }

    fn heights(areas: &Areas) -> Vec<u16> {
        areas.panels.iter().map(|(_, area)| area.height).collect()
    }

    #[test]
    fn large_screen_splits_the_left_column_evenly() {
        let areas = layout(&app(), main(160, 45));

        let heights = heights(&areas);
        assert_eq!(heights.len(), LEFT_PANELS.len());
        assert!(heights.iter().max().unwrap() - heights.iter().min().unwrap() <= 1);
        assert_eq!(heights.iter().sum::<u16>(), 44);
        assert_eq!(areas.panels[0].1.width, 40);
        assert_eq!(areas.detail, Some(Rect::new(40, 0, 120, 44)));
    }

    #[test]
    fn small_screen_collapses_all_but_the_focused_panel() {
        let mut app = app();
        app.focus = Panel::MyPrs;

        let areas = layout(&app, main(80, 24));

        for (panel, area) in &areas.panels {
            if *panel == Panel::MyPrs {
                assert!(area.height >= 10, "{area:?}");
            } else {
                assert_eq!(area.height, 1, "{panel:?}");
            }
        }
        assert_eq!(heights(&areas).iter().sum::<u16>(), 23);
        assert_eq!(areas.panels[0].1.width, 32);
    }

    #[test]
    fn collapsed_layout_keeps_the_last_left_panel_open_while_the_detail_is_focused() {
        let mut app = app();
        app.focus = Panel::Detail;

        let areas = layout(&app, main(80, 24));

        let open: Vec<_> = areas
            .panels
            .iter()
            .filter(|(_, area)| area.height > 1)
            .map(|(panel, _)| *panel)
            .collect();
        assert_eq!(open, [Panel::Inbox]);
    }

    #[test]
    fn zoomed_left_panel_fills_the_left_column() {
        let mut app = app();
        app.focus = Panel::Builds;
        app.zoomed = true;

        let areas = layout(&app, main(160, 45));

        assert_eq!(areas.panels, [(Panel::Builds, Rect::new(0, 0, 40, 44))]);
        assert_eq!(areas.detail, Some(Rect::new(40, 0, 120, 44)));
    }

    #[test]
    fn zoomed_detail_fills_the_screen() {
        let mut app = app();
        app.focus = Panel::Detail;
        app.zoomed = true;

        let areas = layout(&app, main(80, 24));

        assert!(areas.panels.is_empty());
        assert_eq!(areas.detail, Some(main(80, 24)));
    }

    #[test]
    fn left_width_follows_the_config_within_limits() {
        let width = |percent| {
            let mut config = config();
            config.layout.left_percent = percent;
            let mut app = App::new(Some(config), None);
            app.screen = Screen::Main;
            layout(&app, main(200, 45)).panels[0].1.width
        };

        assert_eq!(width(40), 80);
        assert_eq!(width(5), 32);
        assert_eq!(width(90), 120);
    }
}
