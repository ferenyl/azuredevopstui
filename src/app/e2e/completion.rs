use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    MY_PRS, PR_101, assert_only_requests_to, connected, fixture_pull_requests, mark_ready, select,
    wait_for_popup,
};
use crate::app::{CompleteRow, Panel, Popup};

/// My PRs with auto-complete set by me on PR 101.
fn my_prs_with_auto_complete() -> Value {
    let mut prs = fixture_pull_requests("pull-requests-mine.json");
    prs[0]["autoCompleteSetBy"] = json!({ "id": "me-id", "displayName": "Mona Me" });
    json!({ "value": prs })
}

async fn answer_my_prs_with_auto_complete(harness: &Harness) {
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(my_prs_with_auto_complete()),
            1,
        )
        .await;
}

/// Answers a `PATCH` of PR 101 whose body contains all `parts` and asserts it is sent once.
async fn expect_patch_with(harness: &Harness, parts: &[&str]) {
    let mut mock = Mock::given(method("PATCH")).and(path(PR_101));
    for part in parts {
        mock = mock.and(body_string_contains(*part));
    }
    harness
        .ado
        .expect(
            mock,
            ResponseTemplate::new(200).set_body_json(json!({ "pullRequestId": 101 })),
            1,
        )
        .await;
}

fn row(harness: &Harness) -> CompleteRow {
    let Some(Popup::Complete(picker)) = &harness.app.popup else {
        panic!("complete popup not open");
    };
    picker.row()
}

fn go_to(harness: &mut Harness, row_to_find: CompleteRow) {
    for _ in 0..20 {
        if row(harness) == row_to_find {
            return;
        }
        harness.press(KeyCode::Down);
    }
    panic!("row {row_to_find:?} not found");
}

async fn open_popup(harness: &mut Harness) {
    harness.press(KeyCode::Char('c'));
    wait_for_popup(harness, |p| matches!(p, Popup::Complete(_))).await;
    harness.settle().await;
}

async fn confirm(harness: &mut Harness) -> Vec<String> {
    let before = harness.ado.requests().await.len();
    harness.press(KeyCode::Enter);
    harness.settle().await;
    let mut requests = harness.ado.requests().await.split_off(before);
    requests.sort();
    requests
}

#[tokio::test]
async fn auto_complete_is_set_on_a_pull_request_that_is_not_ready() {
    let mut harness = connected().await;
    expect_patch_with(
        &harness,
        &[
            r#""autoCompleteSetBy":{"id":"me-id"}"#,
            r#""deleteSourceBranch":false"#,
        ],
    )
    .await;
    answer_my_prs_with_auto_complete(&harness).await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    open_popup(&mut harness).await;
    assert!(harness.screen().contains("[x] Set auto-complete"));

    let requests = confirm(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_only_requests_to(&requests, &["pullrequests"], &["builds", "wiql"]);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    assert!(
        harness.screen().contains("set by Mona Me"),
        "{}",
        harness.screen()
    );
}

#[tokio::test]
async fn auto_complete_sends_the_chosen_options() {
    let mut harness = connected().await;
    expect_patch_with(
        &harness,
        &[
            r#""deleteSourceBranch":true"#,
            r#""transitionWorkItems":true"#,
        ],
    )
    .await;
    select(&mut harness, Panel::MyPrs, 101);
    open_popup(&mut harness).await;
    go_to(&mut harness, CompleteRow::DeleteSourceBranch);
    harness.press(KeyCode::Char(' '));
    go_to(&mut harness, CompleteRow::TransitionWorkItems);
    harness.press(KeyCode::Char(' '));

    let requests = confirm(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    let completion = harness.app.config.as_ref().unwrap().completion;
    assert!(completion.delete_source_branch);
    assert!(completion.transition_work_items);
}

#[tokio::test]
async fn auto_complete_is_cancelled_with_the_empty_identity() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(my_prs_with_auto_complete()),
            1,
        )
        .await;
    expect_patch_with(
        &harness,
        &[r#""autoCompleteSetBy":{"id":"00000000-0000-0000-0000-000000000000"}"#],
    )
    .await;
    harness.connect().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    assert!(harness.screen().contains("set by Mona Me"));
    open_popup(&mut harness).await;
    go_to(&mut harness, CompleteRow::CancelAutoComplete);

    let requests = confirm(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_only_requests_to(&requests, &["pullrequests"], &["builds", "wiql"]);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    assert!(!harness.screen().contains("set by Mona Me"));
}

#[tokio::test]
async fn completing_sends_the_options_and_the_message() {
    let mut harness = connected().await;
    expect_patch_with(
        &harness,
        &[
            r#""status":"completed""#,
            r#""deleteSourceBranch":true"#,
            r#""mergeCommitMessage":"Merged PR 101: Add order filter""#,
        ],
    )
    .await;
    select(&mut harness, Panel::MyPrs, 101);
    mark_ready(&mut harness, 101);
    open_popup(&mut harness).await;
    assert!(harness.screen().contains("[ ] Set auto-complete"));
    go_to(&mut harness, CompleteRow::DeleteSourceBranch);
    harness.press(KeyCode::Char(' '));

    let requests = confirm(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn bypassing_policies_sends_nothing_until_confirmed() {
    let mut harness = connected().await;
    expect_patch_with(&harness, &[r#""bypassPolicy":true"#, r#""bypassReason""#]).await;
    select(&mut harness, Panel::MyPrs, 101);
    mark_ready(&mut harness, 101);
    open_popup(&mut harness).await;
    go_to(&mut harness, CompleteRow::BypassPolicy);
    harness.press(KeyCode::Char(' '));

    let requests = confirm(&mut harness).await;

    assert!(requests.is_empty(), "{requests:?}");
    assert!(matches!(harness.app.popup, Some(Popup::Confirm { .. })));
    assert!(harness.screen().contains("bypass branch policies"));

    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    let sent = harness.ado.requests().await;
    assert!(sent.contains(&format!("PATCH {PR_101}")), "{sent:?}");
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn declining_the_bypass_sends_nothing() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    mark_ready(&mut harness, 101);
    open_popup(&mut harness).await;
    go_to(&mut harness, CompleteRow::BypassPolicy);
    harness.press(KeyCode::Char(' '));
    confirm(&mut harness).await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Esc);
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert_eq!(harness.ado.requests().await.len(), before);
}

#[tokio::test]
async fn failed_auto_complete_shows_the_error_without_reloading() {
    let mut harness = connected().await;
    harness.ado.fail("PATCH", PR_101, 409).await;
    select(&mut harness, Panel::MyPrs, 101);
    open_popup(&mut harness).await;

    let requests = confirm(&mut harness).await;

    assert!(harness.app.error.is_some());
    assert!(
        requests.iter().all(|r| r.starts_with("PATCH ")),
        "{requests:?}"
    );
}
