use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::wait_for_popup;
use crate::app::{Panel, Popup};

const BUILDS: &str = "/contoso/Fabrikam/_apis/build/builds";
const BUILD_501: &str = "/contoso/Fabrikam/_apis/build/builds/501";

fn build(status: &str) -> Value {
    json!({
        "id": 501,
        "buildNumber": "20261020.1",
        "status": status,
        "definition": { "id": 7, "name": "orders-ci" },
        "sourceBranch": "refs/heads/feature/101",
        "sourceVersion": "deadbeef",
        "repository": { "id": "gh-orders", "type": "GitHub" },
        "queueTime": "2026-10-20T09:30:00Z"
    })
}

/// Connects with build 501 in `status` as the only pipeline run, and selects it.
async fn with_build(status: &str) -> Harness {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(BUILDS)),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [build(status)] })),
            1..,
        )
        .await;
    harness.connect().await;
    harness.wait_for_text("orders-ci").await;
    harness.app.focus = Panel::Builds;
    harness
}

async fn expect_queued(harness: &Harness, body: Value) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(BUILDS))
                .and(body_partial_json(body)),
            ResponseTemplate::new(200).set_body_json(build("notStarted")),
            1,
        )
        .await;
}

async fn open_menu(harness: &mut Harness) {
    harness.press(KeyCode::Char('e'));
    wait_for_popup(harness, |p| matches!(p, Popup::BuildActions { .. })).await;
}

#[tokio::test]
async fn rerunning_queues_the_same_branch_and_commit_after_confirming() {
    let mut harness = with_build("completed").await;
    expect_queued(
        &harness,
        json!({
            "definition": { "id": 7 },
            "sourceBranch": "refs/heads/feature/101",
            "sourceVersion": "deadbeef"
        }),
    )
    .await;
    let before = harness.ado.received("GET", "/build/builds").await;
    open_menu(&mut harness).await;

    harness.press(KeyCode::Enter);
    harness
        .wait_for_text("Re-run orders-ci on feature/101?")
        .await;
    assert_eq!(harness.ado.received("POST", "/build/builds").await, 0);
    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    assert_eq!(harness.ado.received("POST", "/build/builds").await, 1);
    assert!(harness.ado.received("GET", "/build/builds").await > before);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn queueing_asks_for_a_branch_and_confirms() {
    let mut harness = with_build("completed").await;
    expect_queued(
        &harness,
        json!({ "definition": { "id": 7 }, "sourceBranch": "refs/heads/release" }),
    )
    .await;
    open_menu(&mut harness).await;
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    for _ in 0.."feature/101".len() {
        harness.press(KeyCode::Backspace);
    }
    harness.type_text("release");

    harness.press(KeyCode::Enter);
    harness.wait_for_text("Queue orders-ci on release?").await;
    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    assert_eq!(harness.ado.received("POST", "/build/builds").await, 1);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn cancelling_a_running_build_patches_its_status() {
    let mut harness = with_build("inProgress").await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH"))
                .and(path(BUILD_501))
                .and(body_json(json!({ "status": "cancelling" }))),
            ResponseTemplate::new(200).set_body_json(build("cancelling")),
            1,
        )
        .await;
    open_menu(&mut harness).await;
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));

    harness.press(KeyCode::Enter);
    harness.wait_for_text("Cancel orders-ci 20261020.1?").await;
    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    assert_eq!(harness.ado.received("PATCH", "/builds/501").await, 1);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn a_finished_build_cannot_be_cancelled() {
    let mut harness = with_build("completed").await;

    open_menu(&mut harness).await;

    let screen = harness.screen();
    assert!(screen.contains("Re-run (same commit)"), "{screen}");
    assert!(!screen.contains("Cancel run"), "{screen}");
}

#[tokio::test]
async fn declining_sends_nothing() {
    let mut harness = with_build("inProgress").await;

    for (downs, decline) in [(0, KeyCode::Char('n')), (2, KeyCode::Esc)] {
        open_menu(&mut harness).await;
        for _ in 0..downs {
            harness.press(KeyCode::Char('j'));
        }
        harness.press(KeyCode::Enter);
        harness.press(decline);
        assert!(harness.app.popup.is_none());
    }
    open_menu(&mut harness).await;
    harness.press(KeyCode::Esc);
    harness.settle().await;

    assert_eq!(harness.ado.received("POST", "/build/builds").await, 0);
    assert_eq!(harness.ado.received("PATCH", "/builds/501").await, 0);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn a_failed_queue_shows_the_error_without_reloading() {
    let mut harness = with_build("completed").await;
    harness.ado.fail("POST", BUILDS, 403).await;
    let before = harness.ado.received("GET", "/build/builds").await;
    open_menu(&mut harness).await;
    harness.press(KeyCode::Enter);

    harness.press(KeyCode::Char('y'));
    harness.wait_for(|app| app.error.is_some()).await;
    harness.settle().await;

    assert_eq!(harness.ado.received("GET", "/build/builds").await, before);
}
