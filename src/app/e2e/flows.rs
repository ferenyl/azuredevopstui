use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use wiremock::matchers::{
    body_string_contains, method, path, path_regex, query_param, query_param_is_missing,
};
use wiremock::{Mock, ResponseTemplate};

use super::{FIXTURES, Harness, MockAdo};
use crate::api::WorkItem;
use crate::app::{Detail, DetailInfo, Panel, Popup, Screen, SetupStep};

const MY_PRS: &str = "/contoso/Fabrikam/_apis/git/pullrequests";
const BUILDS: &str = "/contoso/Fabrikam/_apis/build/builds";
const CONNECTION_DATA: &str = "/contoso/_apis/connectionData";
const WORK_ITEM_301: &str = "/contoso/Fabrikam/_apis/wit/workitems/301";
const COMMENTS_301: &str = "/contoso/Fabrikam/_apis/wit/workItems/301/comments";

pub(super) async fn connected() -> Harness {
    let mut harness = Harness::new().await;
    harness.connect().await;
    harness
}

/// Focuses `panel` and selects the row of work item or pull request `id`.
pub(super) fn select(harness: &mut Harness, panel: Panel, id: u32) {
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
        Panel::ReadyWorkItems => data
            .ready_work_items
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
async fn push_after_vote_is_flagged_even_with_fractional_seconds() {
    let mut harness = connected().await;
    harness
        .wait_for(|app| {
            app.data
                .other_prs()
                .is_some_and(|prs| prs.iter().any(|pr| pr.pull_request_id == 405))
        })
        .await;

    // 404 was pushed 0.5 s after the vote, 405 was voted 0.5 s after its push.
    let flagged: Vec<u32> = harness
        .app
        .data
        .other_prs()
        .unwrap_or_default()
        .iter()
        .filter(|pr| pr.review.changed_since_vote)
        .map(|pr| pr.pull_request_id)
        .collect();
    assert_eq!(flagged, [404]);
    harness.wait_for_text("!404").await;
    let screen = harness.screen();
    assert!(
        screen
            .lines()
            .any(|line| line.contains("!404") && line.contains('↻')),
        "{screen}"
    );
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
async fn comment_threads_are_newest_first_even_with_fractional_seconds() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.press(KeyCode::Tab);
    harness.wait_for_text("Second look at paging").await;

    // Thread 2 was published 0.5 s after thread 1, so it is listed first.
    let screen = harness.screen();
    let newer = screen.find("Second look at paging").expect("newer thread");
    let older = screen
        .find("Please add a test for the empty filter")
        .expect("older thread");
    assert!(newer < older, "{screen}");
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
    harness.press(KeyCode::Enter);
    harness.settle().await;

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
    harness.press(KeyCode::Enter);
    harness.settle().await;

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
async fn pipelines_are_limited_to_the_last_14_days_and_skip_pull_request_runs() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param_is_missing("minTime")),
            ResponseTemplate::new(500),
            0,
        )
        .await;

    harness.connect().await;

    harness.wait_for_text("orders-ci").await;
    assert!(!harness.screen().contains("pr-validation"));
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

#[tokio::test]
async fn slow_response_for_a_previous_sprint_does_not_replace_the_new_one() {
    const DELAY: Duration = Duration::from_millis(500);
    let mut harness = connected().await;
    // Sprint 40 answers slowly and empty; without generations it would land after Sprint 41.
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/Web/_apis/wit/wiql"))
                .and(body_string_contains("Sprint 40")),
            ResponseTemplate::new(200)
                .set_delay(DELAY)
                .set_body_json(serde_json::json!({ "workItems": [] })),
            0..=2,
        )
        .await;

    for sprint in ["40", "41"] {
        harness.press(KeyCode::Char('i'));
        harness
            .wait_for(|app| matches!(app.popup, Some(Popup::Sprint(_))))
            .await;
        harness.type_text(sprint);
        harness.press(KeyCode::Enter);
    }
    harness
        .wait_for(|app| app.data.sprint_name.as_deref() == Some("Sprint 41"))
        .await;
    tokio::time::sleep(DELAY).await;
    harness.settle().await;

    assert_eq!(harness.app.data.sprint_name.as_deref(), Some("Sprint 41"));
    harness.wait_for_text("Order list paging").await;
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

/// Requests made for the pull request, pipeline and work item lists so far.
async fn list_requests(ado: &MockAdo) -> [usize; 3] {
    [
        ado.received("GET", "/git/pullrequests").await,
        ado.received("GET", "/build/builds").await,
        ado.received("POST", "/wit/wiql").await,
    ]
}

#[tokio::test]
async fn repeated_refresh_does_not_stack_requests() {
    let mut harness = connected().await;
    let before = list_requests(&harness.ado).await;
    harness.press(KeyCode::Char('r'));
    harness.settle().await;
    let once = list_requests(&harness.ado).await;
    // Responses are only handled in settle, so the last two presses come while the first is in flight.
    for _ in 0..3 {
        harness.press(KeyCode::Char('r'));
    }
    harness.settle().await;
    let thrice = list_requests(&harness.ado).await;

    for ((before, once), thrice) in before.iter().zip(once).zip(thrice) {
        assert_eq!(thrice - once, once - before);
    }
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

/// Answers the mentions query with work item 601 whose last comment mentions the user, `loads` times.
async fn mention_601(harness: &Harness, loads: u64) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/Web/_apis/wit/wiql"))
                .and(body_string_contains("@RecentMentions")),
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "workItems": [{ "id": 601 }] })),
            loads,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/_apis/wit/workitemsbatch"))
                .and(body_string_contains("\"ids\":[601]")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [{
                "id": 601,
                "fields": {
                    "System.Title": "Mentioned in checkout",
                    "System.WorkItemType": "Task",
                    "System.State": "Active",
                    "System.ChangedDate": "2026-10-21T10:00:00Z",
                    "System.CreatedDate": "2026-10-01T10:00:00Z"
                }
            }]})),
            loads,
        )
        .await;
}

/// The pull requests of a fixture file.
fn fixture_pull_requests(file: &str) -> Vec<serde_json::Value> {
    let content = std::fs::read_to_string(format!("{FIXTURES}/{file}")).expect("fixture");
    let fixture: serde_json::Value = serde_json::from_str(&content).expect("fixture json");
    fixture["body"]["value"]
        .as_array()
        .expect("pull requests")
        .clone()
}

/// Answers the others' PRs list with `prs` instead of its fixture.
async fn other_pull_requests(harness: &Harness, prs: Vec<serde_json::Value>, times: u64) {
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param_is_missing("searchCriteria.creatorId")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": prs })),
            times,
        )
        .await;
}

/// Requests made by one more refresh after loading settled, sorted.
async fn refresh_requests(harness: &mut Harness) -> Vec<String> {
    let before = harness.ado.requests().await.len();
    harness.app.refresh();
    harness.settle().await;
    let mut requests = harness.ado.requests().await.split_off(before);
    requests.sort();
    requests
}

fn count(requests: &[String], suffix: &str) -> usize {
    requests.iter().filter(|r| r.ends_with(suffix)).count()
}

#[tokio::test]
async fn refresh_of_unchanged_data_skips_cached_requests() {
    let mut harness = Harness::new().await;
    mention_601(&harness, 2).await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/Fabrikam/_apis/wit/workItems/601/comments")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "totalCount": 1,
                "comments": [{
                    "createdBy": { "id": "other-id", "displayName": "Olle Other" },
                    "createdDate": "2026-10-21T10:00:00Z",
                    "text": "ping @<me-id>"
                }]
            })),
            2,
        )
        .await;
    harness.connect().await;

    let requests = refresh_requests(&mut harness).await;

    // Only my PR's checks and the unanswered mention load again; others' PRs and identities are cached.
    assert_eq!(count(&requests, "/threads"), 1, "{requests:#?}");
    assert_eq!(count(&requests, "/pullRequests/101/threads"), 1);
    assert_eq!(count(&requests, "/policy/evaluations"), 1);
    assert_eq!(count(&requests, "/pullRequests/101"), 0);
    assert_eq!(count(&requests, "/build/builds"), 2);
    assert_eq!(count(&requests, "/workItems/601/comments"), 1);
    assert_eq!(requests.len(), 16, "{requests:#?}");
    harness.wait_for_text("Mentioned in checkout").await;
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn refresh_loads_signals_of_changed_pull_requests_again() {
    let mut harness = connected().await;
    let mut prs = fixture_pull_requests("pull-requests-all.json");
    for pr in &mut prs {
        if pr["pullRequestId"] == 202 {
            pr["lastMergeSourceCommit"]["commitId"] = "c202-pushed".into();
        }
    }
    other_pull_requests(&harness, prs, 1).await;

    let requests = refresh_requests(&mut harness).await;

    assert_eq!(
        count(&requests, "/pullRequests/202/threads"),
        1,
        "{requests:#?}"
    );
    assert_eq!(count(&requests, "/pullRequests/404/threads"), 0);
    assert_eq!(
        count(&requests, "/project-fabrikam/_apis/policy/evaluations"),
        2
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn steady_refresh_stays_small_with_many_pull_requests() {
    const PRS: u32 = 40;
    let mut harness = Harness::new().await;
    let template = fixture_pull_requests("pull-requests-all.json")
        .into_iter()
        .find(|pr| pr["pullRequestId"] == 202)
        .expect("PR 202");
    let prs = (1000..1000 + PRS)
        .map(|id| {
            let mut pr = template.clone();
            pr["pullRequestId"] = id.into();
            pr
        })
        .collect();
    other_pull_requests(&harness, prs, 2).await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path_regex(r"/pullRequests/1\d{3}/threads$")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })),
            u64::from(PRS),
        )
        .await;
    harness.connect().await;

    let requests = refresh_requests(&mut harness).await;

    assert_eq!(count(&requests, "/policy/evaluations"), 1, "{requests:#?}");
    assert_eq!(requests.len(), 14, "{requests:#?}");
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn pipelines_load_the_latest_runs_in_one_query() {
    let mut harness = Harness::new().await;
    let build = |id: u32, definition: u32, name: &str, branch: &str, queued: &str| {
        serde_json::json!({
            "id": id,
            "buildNumber": format!("2026102{id}"),
            "status": "completed",
            "result": "succeeded",
            "definition": { "id": definition, "name": name },
            "sourceBranch": branch,
            "repository": { "id": "gh-orders", "type": "GitHub" },
            "queueTime": queued
        })
    };
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("requestedFor", "me-id")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
                build(1, 7, "orders-ci", "refs/heads/feature/101", "2026-10-20T09:30:00Z"),
                build(2, 9, "billing-ci", "refs/heads/main", "2026-10-19T09:30:00Z")
            ]})),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("definitions", "7,9")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [
                build(4, 7, "orders-ci", "refs/heads/feature/101", "2026-10-22T09:30:00.5Z"),
                build(3, 9, "billing-ci", "refs/heads/main", "2026-10-22T09:30:00Z"),
                build(2, 9, "billing-ci", "refs/heads/main", "2026-10-19T09:30:00Z")
            ]})),
            1,
        )
        .await;

    harness.connect().await;

    harness.wait_for_text("billing-ci").await;
    let ids: Vec<u32> = harness
        .app
        .data
        .shown_builds()
        .unwrap_or_default()
        .iter()
        .map(|build| build.id)
        .collect();
    assert_eq!(ids, [4, 3]);
    assert_eq!(harness.ado.received("GET", BUILDS).await, 2);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

/// Presses Enter on the open popup and returns the requests the mutation and its reloads sent.
async fn requests_after_confirming(harness: &mut Harness) -> Vec<String> {
    harness.settle().await;
    let before = harness.ado.requests().await.len();
    harness.press(KeyCode::Enter);
    harness.settle().await;
    let mut requests = harness.ado.requests().await.split_off(before);
    requests.sort();
    requests
}

#[tokio::test]
async fn tagging_without_an_open_detail_reloads_only_the_mentions() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('t'));
    harness
        .wait_for(|app| matches!(app.popup, Some(Popup::Tags(_))))
        .await;
    harness.type_text("frontend");

    let requests = requests_after_confirming(&mut harness).await;

    assert_eq!(
        requests,
        [
            "PATCH /contoso/Fabrikam/_apis/wit/workitems/301",
            "POST /contoso/Fabrikam/Web/_apis/wit/wiql"
        ]
    );
}

#[tokio::test]
async fn tagging_with_the_item_open_reloads_only_its_detail() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Enter);
    harness.press(KeyCode::Char('t'));
    harness
        .wait_for(|app| matches!(app.popup, Some(Popup::Tags(_))))
        .await;
    harness.type_text("frontend");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(requests.contains(&"PATCH /contoso/Fabrikam/_apis/wit/workitems/301".to_string()));
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("GET /contoso/Fabrikam/_apis/wit/workItems/301/comments"))
    );
    for other in ["pullrequests", "builds", "iterations", "workitemsbatch"] {
        assert!(
            requests.iter().all(|r| !r.contains(other)),
            "{other}: {requests:?}"
        );
    }
}

#[tokio::test]
async fn commenting_reloads_only_the_mentions() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('m'));
    harness.type_text("Looks good");

    let requests = requests_after_confirming(&mut harness).await;

    assert_eq!(count(&requests, "/301/comments"), 1);
    assert!(count(&requests, "/wiql") >= 1, "{requests:?}");
    for other in ["pullrequests", "builds", "iterations", "policy"] {
        assert!(
            requests.iter().all(|r| !r.contains(other)),
            "{other}: {requests:?}"
        );
    }
}

const WORK_ITEM_303: &str = "/contoso/Fabrikam/_apis/wit/workitems/303";
const PR_101: &str = "/contoso/project-fabrikam/_apis/git/repositories/repo-api/pullRequests/101";

/// Answers a `PATCH` of `target` containing `body_part` and asserts it is sent once.
async fn expect_patch(harness: &Harness, target: &str, body_part: &str) {
    harness
        .ado
        .expect(
            Mock::given(method("PATCH"))
                .and(path(target))
                .and(body_string_contains(body_part)),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 1 })),
            1,
        )
        .await;
}

/// Lets the signals of PR `id` say it can be completed; the fixture PR lacks a reviewer vote.
fn mark_ready(harness: &mut Harness, id: u32) {
    let mut signals = harness.app.data.pr_signals()[&id];
    signals.ready = true;
    harness.app.data.set_pr_signals(id, signals);
}

async fn wait_for_popup(harness: &mut Harness, popup: fn(&Popup) -> bool) {
    harness
        .wait_for(move |app| app.popup.as_ref().is_some_and(popup))
        .await;
}

fn assert_only_requests_to(requests: &[String], touched: &[&str], untouched: &[&str]) {
    for suffix in touched {
        assert!(
            requests.iter().any(|r| r.contains(suffix)),
            "{suffix}: {requests:?}"
        );
    }
    for other in untouched {
        assert!(
            requests.iter().all(|r| !r.contains(other)),
            "{other}: {requests:?}"
        );
    }
}

#[tokio::test]
async fn assigning_reloads_only_the_sprint() {
    let mut harness = connected().await;
    expect_patch(&harness, WORK_ITEM_303, "/fields/System.AssignedTo").await;
    select(&mut harness, Panel::ReadyWorkItems, 303);
    harness.settle().await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Char('a'));
    harness.settle().await;

    let requests = harness.ado.requests().await.split_off(before);
    assert_eq!(
        requests.first().map(String::as_str),
        Some("PATCH /contoso/Fabrikam/_apis/wit/workitems/303")
    );
    assert_only_requests_to(
        &requests,
        &["/wiql", "workitemsbatch"],
        &["pullrequests", "builds"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn moving_to_another_column_patches_the_column_and_reloads_only_the_sprint() {
    let mut harness = connected().await;
    expect_patch(&harness, WORK_ITEM_301, "Closed").await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('s'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Column(_))).await;
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));

    let requests = requests_after_confirming(&mut harness).await;

    assert!(requests.contains(&"PATCH /contoso/Fabrikam/_apis/wit/workitems/301".to_string()));
    assert_only_requests_to(
        &requests,
        &["/wiql", "workitemsbatch"],
        &["pullrequests", "builds"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn unassigning_clears_the_assignee_and_reloads_only_the_sprint() {
    let mut harness = connected().await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/System.AssignedTo","value":"""#,
    )
    .await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('u'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Column(_))).await;

    let requests = requests_after_confirming(&mut harness).await;

    assert!(requests.contains(&"PATCH /contoso/Fabrikam/_apis/wit/workitems/301".to_string()));
    assert_only_requests_to(&requests, &["/wiql"], &["pullrequests", "builds"]);
}

#[tokio::test]
async fn completing_a_pull_request_reloads_only_the_pull_requests() {
    let mut harness = connected().await;
    expect_patch(&harness, PR_101, r#""status":"completed""#).await;
    select(&mut harness, Panel::MyPrs, 101);
    mark_ready(&mut harness, 101);
    harness.press(KeyCode::Char('c'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Complete(_))).await;

    let requests = requests_after_confirming(&mut harness).await;

    assert!(requests.contains(&format!("PATCH {PR_101}")));
    assert_only_requests_to(
        &requests,
        &["pullrequests"],
        &["builds", "wiql", "workitemsbatch"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn failed_mutations_show_the_error_without_reloading() {
    for status in [409, 500] {
        let mut harness = connected().await;
        harness.ado.fail("PATCH", WORK_ITEM_301, status).await;
        select(&mut harness, Panel::MyWorkItems, 301);
        harness.press(KeyCode::Char('u'));
        wait_for_popup(&mut harness, |p| matches!(p, Popup::Column(_))).await;

        let requests = requests_after_confirming(&mut harness).await;

        assert!(harness.app.error.is_some(), "{status}");
        assert!(
            requests.iter().all(|r| r.starts_with("PATCH ")),
            "{status}: {requests:?}"
        );
    }
}

#[tokio::test]
async fn failed_completion_shows_the_error_without_reloading() {
    let mut harness = connected().await;
    harness.ado.fail("PATCH", PR_101, 409).await;
    select(&mut harness, Panel::MyPrs, 101);
    mark_ready(&mut harness, 101);
    harness.press(KeyCode::Char('c'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Complete(_))).await;

    let requests = requests_after_confirming(&mut harness).await;

    assert!(harness.app.error.is_some());
    assert!(
        requests.iter().all(|r| r.starts_with("PATCH ")),
        "{requests:?}"
    );
}

/// Mounts mention 601 (a User Story) for the next mentions load only; `assigned` makes it mine.
async fn mention_601_once(harness: &Harness, state: &str, assigned: bool) {
    let mut fields = serde_json::json!({
        "System.Title": "Mentioned in checkout",
        "System.WorkItemType": "User Story",
        "System.State": state,
        "System.BoardColumn": state,
        "System.ChangedDate": "2026-10-21T10:00:00Z",
        "System.CreatedDate": "2026-10-01T10:00:00Z"
    });
    if assigned {
        fields["System.AssignedTo"] =
            serde_json::json!({ "id": "me-id", "displayName": "Mona Me" });
    }
    harness
        .ado
        .expect_first(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/Web/_apis/wit/wiql"))
                .and(body_string_contains("@RecentMentions")),
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "workItems": [{ "id": 601 }] })),
            1,
        )
        .await;
    harness
        .ado
        .expect_first(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/_apis/wit/workitemsbatch"))
                .and(body_string_contains("\"ids\":[601]")),
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "value": [{ "id": 601, "fields": fields }] })),
            1,
        )
        .await;
}

async fn mention_601_comment(harness: &Harness) {
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/Fabrikam/_apis/wit/workItems/601/comments")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "totalCount": 1,
                "comments": [{
                    "createdBy": { "id": "other-id", "displayName": "Olle Other" },
                    "createdDate": "2026-10-21T10:00:00Z",
                    "text": "ping @<me-id>"
                }]
            })),
            1..,
        )
        .await;
}

fn select_mention_601(harness: &mut Harness) {
    let row = harness
        .app
        .data
        .inbox()
        .iter()
        .position(|entry| matches!(&entry.detail, Detail::WorkItem(item) if item.id == 601))
        .expect("mention 601 in the inbox");
    harness.app.focus = Panel::Inbox;
    harness.app.selections[Panel::Inbox.index().expect("left panel")] = row;
}

fn mention_601_item(app: &crate::app::App) -> Option<WorkItem> {
    app.data
        .inbox()
        .iter()
        .find_map(|entry| match &entry.detail {
            Detail::WorkItem(item) if item.id == 601 => Some((**item).clone()),
            _ => None,
        })
}

#[tokio::test]
async fn assigning_a_mentioned_item_updates_it_in_the_inbox() {
    let mut harness = Harness::new().await;
    mention_601_once(&harness, "Active", false).await;
    mention_601_once(&harness, "Active", true).await;
    mention_601_comment(&harness).await;
    expect_patch(
        &harness,
        "/contoso/Fabrikam/_apis/wit/workitems/601",
        "System.AssignedTo",
    )
    .await;
    harness.connect().await;
    select_mention_601(&mut harness);
    assert!(
        !harness
            .app
            .is_assigned_to_me(&mention_601_item(&harness.app).expect("mention"))
    );

    harness.press(KeyCode::Char('a'));
    harness
        .wait_for(|app| mention_601_item(app).is_some_and(|item| app.is_assigned_to_me(&item)))
        .await;
}

#[tokio::test]
async fn closing_a_mentioned_item_drops_it_from_the_inbox() {
    let mut harness = Harness::new().await;
    mention_601_once(&harness, "Active", false).await;
    mention_601_comment(&harness).await;
    expect_patch(
        &harness,
        "/contoso/Fabrikam/_apis/wit/workitems/601",
        "Closed",
    )
    .await;
    harness.connect().await;
    select_mention_601(&mut harness);
    harness.press(KeyCode::Char('s'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Column(_))).await;
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));

    harness.press(KeyCode::Enter);

    // The mentions query now falls through to the fixture that finds no open item.
    harness
        .wait_for(|app| mention_601_item(app).is_none())
        .await;
    assert!(mention_601_item(&harness.app).is_none());
    assert!(
        harness
            .app
            .data
            .inbox()
            .iter()
            .all(|entry| !matches!(&entry.detail, Detail::WorkItem(item) if item.id == 601))
    );
}
