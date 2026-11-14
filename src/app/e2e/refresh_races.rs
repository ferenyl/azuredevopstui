//! A list response in flight when one item is refreshed must not bring back the old item, and a
//! full refresh still reloads every list.

use std::time::Duration;

use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    MY_PRS, PR_101, answer_pull_request, connected, fixture_pull_request, fixture_pull_requests,
};
use crate::app::{Load, Message};

const BATCH: &str = "/contoso/Fabrikam/_apis/wit/workitemsbatch";
const BUILDS: &str = "/contoso/Fabrikam/_apis/build/builds";
const BUILD_501: &str = "/contoso/Fabrikam/_apis/build/builds/501";
/// Long enough for the fast answers to arrive first.
const SLOW: Duration = Duration::from_millis(600);

/// Lets the slow response arrive, and everything else settle.
async fn wait_for_slow_response(harness: &mut Harness) {
    tokio::time::sleep(SLOW + Duration::from_millis(300)).await;
    harness.settle().await;
}

/// Lets the request of the list that was just started reach the server before the next one.
async fn started(_harness: &mut Harness) {
    tokio::time::sleep(Duration::from_millis(150)).await;
}

fn my_pr_titles(harness: &Harness) -> Vec<String> {
    harness
        .app
        .data
        .my_prs()
        .unwrap_or_default()
        .iter()
        .filter(|pr| pr.pull_request_id == 101)
        .map(|pr| pr.title.clone())
        .collect()
}

#[tokio::test]
async fn a_pull_request_list_in_flight_does_not_bring_back_the_old_pull_request() {
    let mut harness = connected().await;
    let titled = |title: &str| {
        let mut prs = fixture_pull_requests("pull-requests-mine.json");
        for pr in &mut prs {
            if pr["pullRequestId"] == 101 {
                pr["title"] = json!(title);
            }
        }
        json!({ "value": prs })
    };
    // The first list is slow and still shows the old title; later ones show the new one.
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200)
                .set_body_json(titled("Old title"))
                .set_delay(SLOW),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(titled("New title")),
            1..,
        )
        .await;
    let mut pr = fixture_pull_request("pr-101.json");
    pr["title"] = json!("New title");
    answer_pull_request(&harness, PR_101, pr).await;

    harness.app.reload_only(&[Load::MyPullRequests]);
    started(&mut harness).await;
    harness.app.handle_message(Message::PullRequestUpdated {
        id: 101,
        reload: vec![Load::PullRequest(101)],
        result: Ok(()),
    });
    wait_for_slow_response(&mut harness).await;

    assert_eq!(my_pr_titles(&harness), ["New title"]);
}

#[tokio::test]
async fn a_work_item_list_in_flight_does_not_bring_back_the_old_work_item() {
    let mut harness = connected().await;
    let batch = |title: &str| {
        json!({ "value": [{
            "id": 301,
            "fields": {
                "System.Title": title,
                "System.WorkItemType": "User Story",
                "System.State": "Active",
                "System.ChangedDate": "2026-10-21T10:00:00Z",
                "System.CreatedDate": "2026-10-01T10:00:00Z",
                "System.AssignedTo": { "id": "me-id", "displayName": "Mona Me" }
            }
        }]})
    };
    harness
        .ado
        .expect_first(
            Mock::given(method("POST"))
                .and(path(BATCH))
                .and(body_string_contains("\"ids\":[301,302]")),
            ResponseTemplate::new(200)
                .set_body_json(batch("Old title"))
                .set_delay(SLOW),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(BATCH))
                .and(body_string_contains("\"ids\":[301")),
            ResponseTemplate::new(200).set_body_json(batch("New title")),
            1..,
        )
        .await;

    harness.app.reload_only(&[Load::Sprint]);
    started(&mut harness).await;
    harness.app.handle_message(Message::WorkItemUpdated {
        id: 301,
        reload: vec![Load::WorkItem(301)],
        result: Ok(()),
    });
    wait_for_slow_response(&mut harness).await;

    let titles: Vec<String> = harness
        .app
        .data
        .my_work_items
        .iter()
        .flatten()
        .filter(|item| item.id == 301)
        .map(|item| item.fields.title.clone())
        .collect();
    assert_eq!(titles, ["New title"]);
}

fn build(id: u32, status: &str) -> Value {
    json!({
        "id": id,
        "buildNumber": format!("2026102{}.1", id - 500),
        "status": status,
        "definition": { "id": 7, "name": "orders-ci" },
        "sourceBranch": "refs/heads/feature/101",
        "sourceVersion": "deadbeef",
        "repository": { "id": "gh-orders", "type": "GitHub" },
        "queueTime": format!("2026-10-{}T09:30:00Z", 19 + id - 500)
    })
}

/// Connects with run 501 in progress as the only run. The next list is slow and still shows it;
/// later lists show `fresh`, and a fetch of 501 shows it completed.
async fn with_slow_running_build(fresh: Vec<Value>) -> Harness {
    let mut harness = Harness::new().await;
    let finished = "completed";
    let in_progress = || json!({ "value": [build(501, "inProgress")] });
    // The load on connecting.
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("requestedFor", "me-id")),
            ResponseTemplate::new(200).set_body_json(in_progress()),
            1,
        )
        .await;
    // The refresh that is in flight: slow, and old by the time it arrives.
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("requestedFor", "me-id")),
            ResponseTemplate::new(200)
                .set_body_json(in_progress())
                .set_delay(SLOW),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("requestedFor", "me-id")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": fresh })),
            1..,
        )
        .await;
    // The second step of a list: the latest runs of the pipelines found.
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("definitions", "7")),
            ResponseTemplate::new(200).set_body_json(in_progress()),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(BUILDS))
                .and(query_param("definitions", "7")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": fresh })),
            0..,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(BUILD_501)),
            ResponseTemplate::new(200).set_body_json(build(501, finished)),
            0..,
        )
        .await;
    harness.connect().await;
    harness.wait_for_text("orders-ci").await;
    harness
}

fn build_501_status(harness: &Harness) -> Vec<String> {
    harness
        .app
        .data
        .shown_builds()
        .unwrap_or_default()
        .iter()
        .filter(|build| build.id == 501)
        .map(|build| build.status.clone())
        .collect()
}

#[tokio::test]
async fn a_pipeline_list_in_flight_does_not_bring_back_the_run_before_the_cancel() {
    let mut harness = with_slow_running_build(vec![build(501, "completed")]).await;

    harness.app.reload_only(&[Load::Builds]);
    started(&mut harness).await;
    harness.app.handle_message(Message::BuildUpdated {
        id: 501,
        result: Ok(None),
    });
    wait_for_slow_response(&mut harness).await;

    assert_eq!(build_501_status(&harness), ["completed"]);
}

#[tokio::test]
async fn a_pipeline_list_in_flight_does_not_bring_back_the_run_before_the_rerun() {
    let queued = build(502, "notStarted");
    let mut harness = with_slow_running_build(vec![queued.clone()]).await;
    let queued: crate::api::Build = serde_json::from_value(queued).expect("build");

    harness.app.reload_only(&[Load::Builds]);
    started(&mut harness).await;
    harness.app.handle_message(Message::BuildUpdated {
        id: 501,
        result: Ok(Some(queued)),
    });
    wait_for_slow_response(&mut harness).await;

    let ids: Vec<u32> = harness
        .app
        .data
        .shown_builds()
        .unwrap_or_default()
        .iter()
        .map(|build| build.id)
        .collect();
    assert_eq!(ids, [502]);
}

#[tokio::test]
async fn refreshing_reloads_every_list() {
    let mut harness = connected().await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Char('r'));
    harness.settle().await;

    let requests: Vec<String> = harness.ado.requests().await.split_off(before);
    let count = |needle: &str| requests.iter().filter(|r| r.contains(needle)).count();
    // My PRs and the others' PRs share a path.
    assert!(count("/git/pullrequests") >= 2, "{requests:?}");
    assert!(count("/build/builds") >= 1, "{requests:?}");
    assert!(count("/wit/wiql") >= 1, "{requests:?}");
    assert!(count("/wit/workitemsbatch") >= 1, "{requests:?}");
    // The sprint lists my, ready and mentioned items, each with its own query.
    assert!(count("/wit/wiql") >= 3, "{requests:?}");
}

#[tokio::test]
async fn a_sprint_list_in_flight_is_not_started_twice_by_an_action_that_reloads_it() {
    let mut harness = connected().await;
    harness.app.reload_only(&[Load::Sprint]);
    started(&mut harness).await;

    harness.app.handle_message(Message::WorkItemUpdated {
        id: 301,
        reload: vec![Load::WorkItem(301), Load::Sprint],
        result: Ok(()),
    });

    // Requests are numbered as they start: the sprint is restarted once, right after the item.
    let loads = &harness.app.loads;
    let item = loads.generation(Load::WorkItem(301)).expect("item loading");
    assert_eq!(loads.generation(Load::Sprint), Some(item + 1));
}

#[tokio::test]
async fn a_work_item_that_is_gone_leaves_every_list() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(BATCH))
                .and(body_string_contains("\"ids\":[301]")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [] })),
            1,
        )
        .await;

    harness.app.handle_message(Message::WorkItemUpdated {
        id: 301,
        reload: vec![Load::WorkItem(301)],
        result: Ok(()),
    });
    harness.settle().await;

    let listed = harness
        .app
        .data
        .my_work_items
        .as_deref()
        .unwrap_or_default();
    assert!(listed.iter().all(|item| item.id != 301));
    assert!(!listed.is_empty());
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}
