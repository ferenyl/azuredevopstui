//! Clicks and the wheel.

use crossterm::event::{MouseButton, MouseEventKind};

use super::Harness;
use super::flows::connected;
use super::scroll::pull_request_with_long_description;
use crate::app::{DetailInfo, DetailTab, Panel};

const CLICK: MouseEventKind = MouseEventKind::Down(MouseButton::Left);

/// The cell of the first character of row `row` of `panel`.
fn row_cell(harness: &Harness, panel: Panel, row: u16) -> (u16, u16) {
    harness.screen();
    let areas = harness.app.areas.borrow();
    let hit = areas
        .panels
        .iter()
        .find(|hit| hit.panel == panel)
        .expect("panel drawn");
    (hit.rows.x + 1, hit.rows.y + row)
}

#[tokio::test]
async fn clicking_rows_selects_then_opens_and_clicking_a_tab_switches_it() {
    let mut harness = connected().await;
    assert!(harness.app.data.len(Panel::OtherPrs) >= 2);
    let (x, y) = row_cell(&harness, Panel::OtherPrs, 1);
    harness.mouse(CLICK, x, y);
    assert_eq!(harness.app.focus, Panel::OtherPrs);
    assert_eq!(harness.app.selections[3], 1);
    assert!(harness.app.detail.is_none());

    let (x, y) = row_cell(&harness, Panel::MyPrs, 0);
    harness.mouse(CLICK, x, y);
    assert_eq!(harness.app.focus, Panel::MyPrs);
    assert!(harness.app.detail.is_none());

    harness.mouse(CLICK, x, y);
    assert!(harness.app.detail.is_some());
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;

    harness.screen();
    let comments = harness
        .app
        .areas
        .borrow()
        .tabs
        .iter()
        .find(|(tab, _)| *tab == DetailTab::Comments)
        .map(|(_, area)| *area)
        .expect("comments tab drawn");
    harness.mouse(CLICK, comments.x + 1, comments.y);

    assert_eq!(harness.app.detail_tab, DetailTab::Comments);
    assert_eq!(harness.app.focus, Panel::Detail);
}

#[tokio::test]
async fn clicking_a_panel_on_a_small_terminal_expands_it() {
    let mut harness = connected().await;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).expect("terminal");
    terminal
        .draw(|frame| crate::ui::render(frame, &harness.app))
        .expect("render");
    let title = harness
        .app
        .areas
        .borrow()
        .panels
        .iter()
        .find(|hit| hit.panel == Panel::MyPrs)
        .map(|hit| hit.area)
        .expect("panel drawn");
    assert_eq!(title.height, 1);

    harness.app.handle_mouse(crossterm::event::MouseEvent {
        kind: CLICK,
        column: title.x + 2,
        row: title.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    let screen = harness.screen_at(80, 24);

    assert_eq!(harness.app.focus, Panel::MyPrs);
    assert!(screen.contains("│!101  ✎2  Add order filter"), "{screen}");
}

#[tokio::test]
async fn wheel_over_the_detail_scrolls_it() {
    let mut harness = pull_request_with_long_description().await;
    harness.screen();
    let detail = harness
        .app
        .areas
        .borrow()
        .panels
        .iter()
        .find(|hit| hit.panel == Panel::Detail)
        .map(|hit| hit.area)
        .expect("detail drawn");
    let (x, y) = (detail.x + 5, detail.y + 10);

    harness.mouse(MouseEventKind::ScrollDown, x, y);
    assert_eq!(harness.app.detail_scroll, 3);
    for _ in 0..100 {
        harness.mouse(MouseEventKind::ScrollDown, x, y);
    }
    assert_eq!(
        harness.app.detail_scroll,
        harness.app.detail_max_scroll.get()
    );
    assert!(harness.screen().contains("Line 60"));

    harness.mouse(MouseEventKind::ScrollUp, x, y);
    assert_eq!(
        harness.app.detail_scroll,
        harness.app.detail_max_scroll.get() - 3
    );
}

#[tokio::test]
async fn wheel_over_a_list_moves_its_selection() {
    let mut harness = connected().await;
    let last = harness.app.data.len(Panel::OtherPrs) - 1;
    assert!(last > 0);
    let (x, y) = row_cell(&harness, Panel::OtherPrs, 0);

    for _ in 0..=last + 2 {
        harness.mouse(MouseEventKind::ScrollDown, x, y);
    }

    assert_eq!(harness.app.selections[3], last);
    assert_eq!(harness.app.focus, Panel::Inbox);
}
