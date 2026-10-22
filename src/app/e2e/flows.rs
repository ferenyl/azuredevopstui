use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use wiremock::matchers::{body_string_contains, method, path, path_regex};
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
        Panel::OtherPrs => data
            .other_prs()
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
async fn pull_request_rejected_by_a_reviewer_is_listed_not_approved() {
    let mut harness = connected().await;
    harness
        .wait_for(|app| {
            app.data
                .other_prs()
                .is_some_and(|prs| prs.iter().any(|pr| pr.pull_request_id == 303))
        })
        .await;

    let shown = harness.app.data.shown_other_prs().unwrap_or_default();
    assert!(shown.iter().any(|pr| pr.pull_request_id == 303));
    harness.wait_for_text("Reviewer rejected CSV export").await;
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
async fn pull_request_markdown_is_rendered_once_not_per_frame() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    let renders = || crate::rich_text::MARKDOWN_RENDERS.with(|count| count.get());
    let before = renders();

    let first = harness.screen();
    for _ in 0..10 {
        harness.press(KeyCode::Down);
        harness.screen();
    }

    assert!(first.contains("Adds a status filter to the order list."));
    assert_eq!(renders(), before);
}

#[tokio::test]
async fn pull_request_from_another_project_loads_details_and_links_to_its_project() {
    let mut harness = connected().await;
    harness.wait_for(|app| app.data.other_prs().is_some()).await;
    select(&mut harness, Panel::OtherPrs, 303);

    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;

    assert_eq!(
        harness.app.current_url().as_deref(),
        Some("https://dev.azure.com/contoso/Other/_git/orders-api/pullrequest/303")
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

#[tokio::test]
async fn throttled_pull_requests_are_retried() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .fail_once(
            "GET",
            MY_PRS,
            ResponseTemplate::new(429).insert_header("retry-after", "0"),
        )
        .await;

    harness.connect().await;

    harness.wait_for_text("Add order filter").await;
    harness.wait_for_text("Fix login redirect").await;
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn unavailable_builds_are_retried() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .fail_once("GET", BUILDS, ResponseTemplate::new(503))
        .await;

    harness.connect().await;

    harness.wait_for_text("orders-ci").await;
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn rejected_pat_is_not_retried() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(CONNECTION_DATA)),
            ResponseTemplate::new(401),
            1,
        )
        .await;

    harness.connect().await;

    assert!(matches!(
        harness.app.screen,
        Screen::Setup(SetupStep::EnterPat { error: Some(_), .. })
    ));
}

#[tokio::test]
async fn tagging_is_not_retried_when_unavailable() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH")).and(path(WORK_ITEM_301)),
            ResponseTemplate::new(503).set_body_json(serde_json::json!({ "message": "busy" })),
            1,
        )
        .await;
    select(&mut harness, Panel::MyWorkItems, 301);

    harness.press(KeyCode::Char('t'));
    harness
        .wait_for(|app| matches!(app.popup, Some(Popup::Tags(_))))
        .await;
    harness.type_text("frontend");
    harness.press(KeyCode::Enter);
    harness.wait_for(|app| app.error.is_some()).await;
    harness.settle().await;

    let error = harness.app.error.clone().unwrap_or_default();
    assert!(error.contains("503"), "{error}");
}

#[tokio::test]
async fn many_sub_requests_run_in_bounded_waves() {
    const ITEMS: u32 = 40;
    const DELAY: Duration = Duration::from_millis(100);
    let mut harness = Harness::new().await;
    let ids: Vec<u32> = (500..500 + ITEMS).collect();
    let work_items: Vec<_> = ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "fields": {
                    "System.Title": format!("Mentioned {id}"),
                    "System.WorkItemType": "Task",
                    "System.State": "Active",
                    "System.ChangedDate": "2026-10-23T10:00:00Z",
                    "System.CreatedDate": "2026-10-01T10:00:00Z"
                }
            })
        })
        .collect();
    let wiql_items: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({ "id": id }))
        .collect();
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/Web/_apis/wit/wiql"))
                .and(body_string_contains("@RecentMentions")),
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "workItems": wiql_items })),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/_apis/wit/workitemsbatch"))
                .and(body_string_contains("\"ids\":[500")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": work_items })),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path_regex(r"/wit/workItems/5\d\d/comments$")),
            ResponseTemplate::new(200)
                .set_delay(DELAY)
                .set_body_json(serde_json::json!({ "totalCount": 0, "comments": [] })),
            u64::from(ITEMS),
        )
        .await;

    let started = Instant::now();
    harness.connect().await;
    while harness.ado.received("GET", "/comments").await < ITEMS as usize {
        harness.settle().await;
    }

    // With at most 8 in flight, the last request starts after four full waves of DELAY.
    assert!(
        started.elapsed() >= DELAY * 4,
        "40 delayed requests were not bounded: {:?}",
        started.elapsed()
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}
