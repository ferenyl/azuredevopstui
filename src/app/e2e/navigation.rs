use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::flows::connected;
use super::scroll::pull_request_with_long_description;
use crate::app::Panel;

#[tokio::test]
async fn jump_keys_and_wrapping_move_through_a_list() {
    let mut harness = connected().await;
    harness.press(KeyCode::Char('4'));
    assert_eq!(harness.app.focus, Panel::OtherPrs);
    let last = harness.app.data.len(Panel::OtherPrs) - 1;
    assert!(last > 0);

    harness.key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(harness.app.selections[3], last);
    harness.press(KeyCode::Char('j'));
    assert_eq!(harness.app.selections[3], 0);
    harness.press(KeyCode::Char('k'));
    assert_eq!(harness.app.selections[3], last);
    harness.press(KeyCode::Char('g'));
    assert_eq!(harness.app.selections[3], 0);
    harness.press(KeyCode::PageDown);
    assert_eq!(harness.app.selections[3], last);
    harness.press(KeyCode::PageUp);
    assert_eq!(harness.app.selections[3], 0);
}

#[tokio::test]
async fn ctrl_d_moves_half_a_page_and_number_keys_switch_box() {
    let mut harness = connected().await;
    harness.press(KeyCode::Char('4'));
    harness.screen();
    let page = usize::from(harness.app.list_height.get());
    assert!(page > 0);

    harness.ctrl('d');
    let last = harness.app.data.len(Panel::OtherPrs) - 1;
    assert_eq!(harness.app.selections[3], (page / 2).max(1).min(last));
    harness.ctrl('u');
    assert_eq!(harness.app.selections[3], 0);

    harness.press(KeyCode::Char('2'));
    assert_eq!(harness.app.focus, Panel::MyPrs);
}

#[tokio::test]
async fn jump_keys_scroll_the_detail() {
    let mut harness = pull_request_with_long_description().await;
    harness.app.focus = Panel::Detail;
    harness.screen();
    let max = harness.app.detail_max_scroll.get();
    let page = harness.app.detail_height.get();
    assert!(max > page);

    harness.key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(harness.app.detail_scroll, max);
    assert!(harness.screen().contains("Line 60"));

    harness.press(KeyCode::Char('g'));
    assert_eq!(harness.app.detail_scroll, 0);
    harness.ctrl('d');
    assert_eq!(harness.app.detail_scroll, page / 2);
    harness.press(KeyCode::PageDown);
    assert_eq!(harness.app.detail_scroll, page / 2 + page);
}
