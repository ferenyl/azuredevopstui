use crossterm::event::KeyCode;

use super::flows::{connected, expect_patch, select, wait_for_popup};
use crate::app::{Panel, Popup};

const WORK_ITEM_303: &str = "/contoso/Fabrikam/_apis/wit/workitems/303";

async fn open_tags_of_303(harness: &mut super::Harness) {
    select(harness, Panel::ReadyWorkItems, 303);
    harness.press(KeyCode::Char('t'));
    wait_for_popup(harness, |p| matches!(p, Popup::Tags(_))).await;
}

#[tokio::test]
async fn several_tags_of_an_item_that_is_not_mine_go_in_one_patch() {
    let mut harness = connected().await;
    expect_patch(
        &harness,
        WORK_ITEM_303,
        r#""value":"backend; frontend; ops""#,
    )
    .await;
    open_tags_of_303(&mut harness).await;

    harness.press(KeyCode::Tab);
    harness.press(KeyCode::Down);
    harness.press(KeyCode::Tab);
    harness.type_text("ops");
    harness.press(KeyCode::Tab);
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    assert_eq!(harness.ado.received("PATCH", WORK_ITEM_303).await, 1);
    assert_eq!(harness.config().recent_tags, ["backend", "frontend", "ops"]);
}

#[tokio::test]
async fn the_popup_shows_checked_tags() {
    let mut harness = connected().await;
    open_tags_of_303(&mut harness).await;

    harness.press(KeyCode::Tab);

    let screen = harness.screen();
    assert!(screen.contains("[x] backend"), "{screen}");
    assert!(screen.contains("[ ] frontend"), "{screen}");
}

#[tokio::test]
async fn esc_sends_nothing() {
    let mut harness = connected().await;
    open_tags_of_303(&mut harness).await;
    harness.press(KeyCode::Tab);

    harness.press(KeyCode::Esc);
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert_eq!(harness.ado.received("PATCH", WORK_ITEM_303).await, 0);
}
