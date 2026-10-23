use std::time::Duration;

use crossterm::event::KeyCode;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::flows::{PR_101, connected, select, wait_for_popup};
use crate::app::{Panel, Popup};

const WORK_ITEM_303: &str = "/contoso/Fabrikam/_apis/wit/workitems/303";

#[tokio::test]
async fn assigning_shows_the_action_in_flight_and_then_its_result() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH")).and(path(WORK_ITEM_303)),
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "id": 303 }))
                .set_delay(Duration::from_millis(500)),
            1,
        )
        .await;
    select(&mut harness, Panel::ReadyWorkItems, 303);

    harness.press(KeyCode::Char('a'));

    assert!(
        harness.screen().contains("assigning #303…"),
        "{}",
        harness.screen()
    );
    assert!(!harness.screen().contains("assigning #303 ✓"));
    harness.wait_for_text("assigning #303 ✓").await;
    assert!(!harness.screen().contains("assigning #303…"));
}

#[tokio::test]
async fn failed_action_is_listed_in_the_errors_popup_after_a_refresh() {
    let mut harness = connected().await;
    let reviewer = format!("{PR_101}/reviewers/me-id");
    harness.ado.fail("PUT", &reviewer, 500).await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('v'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
    harness.press(KeyCode::Enter);
    harness.wait_for(|app| app.error.is_some()).await;
    let error = harness.app.error.clone().expect("error");
    assert!(
        harness.screen().contains("[E] errors (1)"),
        "{}",
        harness.screen()
    );
    assert!(harness.screen().contains(&error));

    harness.press(KeyCode::Char('r'));
    harness.settle().await;

    assert_eq!(harness.app.error, None);
    assert!(harness.screen().contains("[E] errors (1)"));
    harness.press(KeyCode::Char('E'));
    let screen = harness.screen();
    assert!(screen.contains("Errors (1)"), "{screen}");
    assert!(screen.contains(&error), "{screen}");
    harness.press(KeyCode::Esc);
    assert!(harness.app.popup.is_none());
}
