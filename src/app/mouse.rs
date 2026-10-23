use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::input::Move;
use super::{Action, App, DetailTab, Panel, Screen};

/// Rows the detail panel scrolls per wheel step.
const WHEEL_ROWS: usize = 3;

/// A panel as last drawn.
#[derive(Debug, Clone, Copy)]
pub struct PanelHit {
    pub panel: Panel,
    pub area: Rect,
    /// Where the rows are drawn, inside the border.
    pub rows: Rect,
    /// Index of the first drawn row.
    pub offset: usize,
}

/// A list of choices (popup or setup step) as last drawn.
#[derive(Debug, Clone, Copy)]
pub struct PickerHit {
    pub rows: Rect,
    pub offset: usize,
    pub len: usize,
    pub selected: usize,
}

/// Where things were drawn in the last frame, for turning mouse positions into targets.
#[derive(Debug, Default)]
pub struct HitAreas {
    pub panels: Vec<PanelHit>,
    pub tabs: Vec<(DetailTab, Rect)>,
    pub picker: Option<PickerHit>,
}

impl PanelHit {
    /// The row index under `position`, if it is on a drawn row.
    fn row_at(&self, position: Position, len: usize) -> Option<usize> {
        if !self.rows.contains(position) {
            return None;
        }
        let row = self.offset + usize::from(position.y - self.rows.y);
        (row < len).then_some(row)
    }
}

impl PickerHit {
    fn row_at(&self, position: Position) -> Option<usize> {
        if !self.rows.contains(position) {
            return None;
        }
        let row = self.offset + usize::from(position.y - self.rows.y);
        (row < self.len).then_some(row)
    }
}

impl App {
    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.show_help {
            return;
        }
        let position = Position::new(mouse.column, mouse.row);
        let wheel_down = match mouse.kind {
            MouseEventKind::ScrollDown => Some(true),
            MouseEventKind::ScrollUp => Some(false),
            _ => None,
        };
        let click = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        if self.popup.is_some() || matches!(self.screen, Screen::Setup(_)) {
            self.mouse_in_picker(position, wheel_down, click);
        } else if let Some(down) = wheel_down {
            self.wheel(position, down);
        } else if click {
            self.click(position);
        }
    }

    fn mouse_in_picker(&mut self, position: Position, wheel_down: Option<bool>, click: bool) {
        let Some(picker) = self.areas.borrow().picker else {
            return;
        };
        if let Some(down) = wheel_down {
            self.apply(if down { Action::Down } else { Action::Up });
        } else if click && let Some(row) = picker.row_at(position) {
            let action = if row >= picker.selected {
                Action::Down
            } else {
                Action::Up
            };
            for _ in 0..row.abs_diff(picker.selected) {
                self.apply(action);
            }
            self.apply(Action::Confirm);
        }
    }

    fn wheel(&mut self, position: Position, down: bool) {
        let Some(hit) = self.panel_at(position) else {
            return;
        };
        match hit.panel.index() {
            None => {
                for _ in 0..WHEEL_ROWS {
                    self.scroll_detail(down);
                }
            }
            Some(index) => {
                let len = self.data.len(hit.panel);
                let selected = self.selections[index];
                self.selections[index] = Move::Line { down }.target(selected, len, 1);
            }
        }
    }

    fn click(&mut self, position: Position) {
        let tab = self
            .areas
            .borrow()
            .tabs
            .iter()
            .find(|(_, area)| area.contains(position))
            .map(|(tab, _)| *tab);
        if let Some(tab) = tab {
            self.focus_panel(Panel::Detail);
            self.select_tab(tab);
            return;
        }
        let Some(hit) = self.panel_at(position) else {
            return;
        };
        let was_focused = self.focus == hit.panel;
        self.focus_panel(hit.panel);
        let Some(index) = hit.panel.index() else {
            return;
        };
        let Some(row) = hit.row_at(position, self.data.len(hit.panel)) else {
            return;
        };
        let reopened = was_focused && self.selections[index] == row;
        self.selections[index] = row;
        if reopened {
            self.open_selected();
        }
    }

    fn panel_at(&self, position: Position) -> Option<PanelHit> {
        self.areas
            .borrow()
            .panels
            .iter()
            .find(|hit| hit.area.contains(position))
            .copied()
    }

    fn focus_panel(&mut self, panel: Panel) {
        self.last_left = self.left_focus();
        self.focus = panel;
        if panel != Panel::Detail {
            self.last_left = panel;
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::test_helpers::{app, open_pull_request, press};
    use crate::app::{App, DetailTab, Panel, Popup};
    use crossterm::event::KeyCode;

    fn draw(app: &App, width: u16, height: u16) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, app))
            .unwrap();
    }

    fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(app: &mut App, column: u16, row: u16) {
        mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    }

    /// Top-left row of `panel`'s list.
    fn first_row(app: &App, panel: Panel) -> (u16, u16) {
        let hit = app
            .areas
            .borrow()
            .panels
            .iter()
            .find(|hit| hit.panel == panel)
            .copied()
            .expect("panel drawn");
        (hit.rows.x + 1, hit.rows.y)
    }

    #[test]
    fn clicking_a_row_focuses_the_panel_and_selects_it() {
        let mut app = app();
        app.focus = Panel::Inbox;
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::MyPrs);

        click(&mut app, x, y + 2);

        assert_eq!(app.focus, Panel::MyPrs);
        assert_eq!(app.selections[1], 2);
        assert!(app.detail.is_none());
    }

    #[test]
    fn clicking_the_selected_row_again_opens_it() {
        let mut app = app();
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::MyPrs);

        click(&mut app, x, y + 1);
        click(&mut app, x, y + 1);

        assert!(app.detail.as_ref().is_some_and(|d| d.is_pull_request(2)));
    }

    #[test]
    fn clicking_below_the_last_row_only_focuses() {
        let mut app = app();
        app.focus = Panel::Inbox;
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::MyPrs);

        click(&mut app, x, y + 5);

        assert_eq!(app.focus, Panel::MyPrs);
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn rows_are_offset_when_the_list_scrolled() {
        let mut app = app();
        app.data.set_my_prs(
            (1..=30)
                .map(|id| crate::test_support::pull_request(id, "PR", "me", "2026-10-03T10:00:00Z"))
                .collect(),
        );
        app.selections[1] = 25;
        draw(&app, 160, 45);
        let hit = app
            .areas
            .borrow()
            .panels
            .iter()
            .find(|h| h.panel == Panel::MyPrs)
            .copied();
        let hit = hit.unwrap();
        assert_eq!(hit.offset, 25 + 1 - usize::from(hit.rows.height));

        click(&mut app, hit.rows.x + 1, hit.rows.y);

        assert_eq!(app.selections[1], hit.offset);
    }

    #[test]
    fn collapsed_panels_are_focused_by_their_title() {
        let mut app = app();
        app.focus = Panel::Inbox;
        draw(&app, 80, 24);
        let hit = app
            .areas
            .borrow()
            .panels
            .iter()
            .find(|h| h.panel == Panel::MyPrs)
            .copied()
            .unwrap();
        assert_eq!(hit.area.height, 1);

        click(&mut app, hit.area.x + 2, hit.area.y);

        assert_eq!(app.focus, Panel::MyPrs);
    }

    #[test]
    fn hidden_panels_are_not_hit_when_zoomed() {
        let mut app = app();
        app.zoomed = true;
        draw(&app, 160, 45);

        assert_eq!(
            app.areas
                .borrow()
                .panels
                .iter()
                .filter(|h| h.panel != Panel::Detail)
                .count(),
            1
        );
    }

    #[test]
    fn clicking_a_tab_shows_it_and_focuses_the_detail() {
        let mut app = app();
        open_pull_request(&mut app);
        draw(&app, 160, 45);
        let comments = app
            .areas
            .borrow()
            .tabs
            .iter()
            .find(|(tab, _)| *tab == DetailTab::Comments)
            .map(|(_, area)| *area)
            .unwrap();

        click(&mut app, comments.x + 2, comments.y);

        assert_eq!(app.detail_tab, DetailTab::Comments);
        assert_eq!(app.focus, Panel::Detail);
        assert_eq!(app.left_focus(), Panel::MyPrs);
    }

    #[test]
    fn clicking_in_the_detail_focuses_it() {
        let mut app = app();
        open_pull_request(&mut app);
        press(&mut app, KeyCode::Char('1'));
        draw(&app, 160, 45);

        click(&mut app, 100, 30);

        assert_eq!(app.focus, Panel::Detail);
    }

    #[test]
    fn wheel_moves_the_selection_of_the_panel_under_the_mouse_and_stops_at_the_ends() {
        let mut app = app();
        app.focus = Panel::Inbox;
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::MyPrs);

        for _ in 0..5 {
            mouse(&mut app, MouseEventKind::ScrollDown, x, y);
        }
        assert_eq!(app.selections[1], 2);
        assert_eq!(app.focus, Panel::Inbox);
        for _ in 0..5 {
            mouse(&mut app, MouseEventKind::ScrollUp, x, y);
        }
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn wheel_scrolls_the_detail_by_three_and_clamps() {
        let mut app = app();
        open_pull_request(&mut app);
        app.detail_max_scroll.set(5);
        draw(&app, 160, 45);
        app.detail_max_scroll.set(5);

        mouse(&mut app, MouseEventKind::ScrollDown, 100, 20);
        assert_eq!(app.detail_scroll, 3);
        mouse(&mut app, MouseEventKind::ScrollDown, 100, 20);
        assert_eq!(app.detail_scroll, 5);
        mouse(&mut app, MouseEventKind::ScrollUp, 100, 20);
        assert_eq!(app.detail_scroll, 2);
    }

    #[test]
    fn popup_wheel_moves_and_click_selects_and_confirms() {
        let mut app = app();
        press(&mut app, KeyCode::Char('S'));
        draw(&app, 160, 45);
        let rows = app.areas.borrow().picker.expect("picker drawn").rows;

        mouse(&mut app, MouseEventKind::ScrollDown, rows.x, rows.y);
        let Some(Popup::Sort { selection, .. }) = &app.popup else {
            panic!("sort picker not open");
        };
        assert_eq!(selection.selected, 1);

        click(&mut app, rows.x + 1, rows.y + 3);

        assert!(app.popup.is_none());
        assert_eq!(
            app.config.as_ref().unwrap().sort.pull_requests,
            crate::config::PrSort::Repository
        );
    }

    #[test]
    fn clicks_outside_an_open_popup_are_ignored() {
        let mut app = app();
        press(&mut app, KeyCode::Char('S'));
        assert!(app.popup.is_some());
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::Inbox);

        click(&mut app, x, y);

        assert!(app.popup.is_some());
        assert_eq!(app.focus, Panel::MyPrs);
    }

    #[test]
    fn mouse_is_ignored_over_the_help() {
        let mut app = app();
        app.focus = Panel::Inbox;
        press(&mut app, KeyCode::Char('?'));
        draw(&app, 160, 45);
        let (x, y) = first_row(&app, Panel::MyPrs);

        click(&mut app, x, y);

        assert_eq!(app.focus, Panel::Inbox);
    }

    #[test]
    fn positions_outside_every_area_do_nothing() {
        let mut app = app();
        draw(&app, 160, 45);

        click(&mut app, 159, 44);

        assert_eq!(app.focus, Panel::MyPrs);
        assert!(!app.areas.borrow().panels.is_empty());
    }
}
