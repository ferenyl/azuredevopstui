use crossterm::event::KeyCode;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use crate::app::{DetailInfo, Panel, Popup, Screen, SetupStep};

const MY_PRS: &str = "/contoso/Fabrikam/_apis/git/pullrequests";
const BUILDS: &str = "/contoso/Fabrikam/_apis/build/builds";
const CONNECTION_DATA: &str = "/contoso/_apis/connectionData";
const WORK_ITEM_301: &str = "/contoso/Fabrikam/_apis/wit/workitems/301";
const COMMENTS_301: &str = "/contoso/Fabrikam/_apis/wit/workItems/301/comments";

async fn connected() -> Harness {
    let mut harness = Harness::new().await;
    harness.connect().await;
    harness
}

/// Focuses `panel` and selects the row of work item or pull request `id`.
fn select(harness: &mut Harness, panel: Panel, id: u32) {
    let index = panel.index().expect("left panel");
    let data = &harness.app.data;
    let row = match panel {
        Panel::MyPrs => data
            .my_prs()
            .and_then(|prs| prs.iter().position(|pr| pr.pull_request_id == id)),
        Panel::MyWorkItems => data
            .my_work_items
            .as_ref()
            .and_then(|items| items.iter().position(|item| item.id == id)),
        _ => None,
    };
    harness.app.focus = panel;
    harness.app.selections[index] = row.expect("row for id");
}

#[tokio::test]
async fn startup_loads_pull_requests_work_items_and_builds() {
    let mut harness = connected().await;

    assert!(harness.is_main());
    for text in [
        "Add order filter",
        "Fix login redirect",
        "Order list paging",
        "Crash on empty cart",
        "Export orders to CSV",
        "orders-ci",
        "Sprint 41",
    ] {
        harness.wait_for_text(text).await;
    }
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn opening_a_pull_request_loads_its_details() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);

    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;

    let screen = harness.screen();
    assert!(
        screen.contains("Adds a status filter to the order list."),
        "{screen}"
    );
}

#[tokio::test]
async fn tagging_a_work_item_patches_its_tags() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH"))
                .and(path(WORK_ITEM_301))
                .and(body_string_contains(r#""value":"backend; frontend""#)),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 301 })),
            1,
        )
        .await;
    select(&mut harness, Panel::MyWorkItems, 301);

    harness.press(KeyCode::Char('t'));
    harness
        .wait_for(|app| matches!(app.popup, Some(Popup::Tags(_))))
        .await;
    harness.type_text("frontend");
    let before = harness.app.last_refresh;
    harness.press(KeyCode::Enter);
    // A successful update refreshes the lists.
    harness.wait_for(|app| app.last_refresh > before).await;

    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    assert_eq!(harness.config().recent_tags, ["frontend"]);
}

#[tokio::test]
async fn commenting_on_a_work_item_posts_the_comment() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(COMMENTS_301))
                .and(body_string_contains("Looks good")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 2 })),
            1,
        )
        .await;
    select(&mut harness, Panel::MyWorkItems, 301);

    harness.press(KeyCode::Char('m'));
    harness.type_text("Looks good");
    let before = harness.app.last_refresh;
    harness.press(KeyCode::Enter);
    // A successful update refreshes the lists.
    harness.wait_for(|app| app.last_refresh > before).await;

    assert!(harness.app.popup.is_none());
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn rejected_pat_asks_for_a_new_one() {
    let mut harness = Harness::new().await;
    harness.ado.fail("GET", CONNECTION_DATA, 401).await;

    harness.connect().await;

    assert!(matches!(
        harness.app.screen,
        Screen::Setup(SetupStep::EnterPat { error: Some(_), .. })
    ));
    assert!(harness.app.client.is_none());
    let screen = harness.screen();
    assert!(screen.contains("The saved token was rejected"), "{screen}");
}

#[tokio::test]
async fn rejected_pat_during_refresh_asks_for_a_new_one() {
    let mut harness = Harness::new().await;
    harness.ado.fail("GET", MY_PRS, 401).await;

    harness.connect().await;

    harness
        .wait_for(|app| matches!(app.screen, Screen::Setup(SetupStep::EnterPat { .. })))
        .await;
}

#[tokio::test]
async fn server_error_shows_error_and_keeps_other_panels() {
    let mut harness = Harness::new().await;
    harness.ado.fail("GET", BUILDS, 500).await;

    harness.connect().await;

    harness.wait_for(|app| app.error.is_some()).await;
    let error = harness.app.error.clone().unwrap_or_default();
    assert!(error.starts_with("500"), "{error}");
    assert!(harness.is_main());
    harness.wait_for_text("Add order filter").await;
    harness.wait_for_text("Order list paging").await;
}
