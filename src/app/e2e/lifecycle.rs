use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    MY_PRS, PR_101, assert_only_requests_to, connected, expect_patch, fixture_pull_requests,
    requests_after_confirming, select, wait_for_popup,
};
use crate::app::{Panel, Popup};

const REVIEWER_ANNA: &str =
    "/contoso/project-fabrikam/_apis/git/repositories/repo-api/pullRequests/101/reviewers/anna-id";
const WORK_ITEM_301: &str = "/contoso/Fabrikam/_apis/wit/workitems/301";

/// A harness whose first load of my PRs has PR 101 changed by `change`.
async fn with_changed_pr(change: impl Fn(&mut Value)) -> Harness {
    let mut harness = Harness::new().await;
    let mut prs = fixture_pull_requests("pull-requests-mine.json");
    change(&mut prs[0]);
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": prs })),
            1,
        )
        .await;
    harness.connect().await;
    harness
}

fn anna_has_an_id(pr: &mut Value) {
    pr["reviewers"][0]["id"] = json!("anna-id");
}

/// Opens the menu of PR 101 and picks the entry `downs` rows down.
async fn pick_action(harness: &mut Harness, downs: usize) {
    select(harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('e'));
    wait_for_popup(harness, |p| matches!(p, Popup::PrActions { .. })).await;
    for _ in 0..downs {
        harness.press(KeyCode::Char('j'));
    }
    harness.press(KeyCode::Enter);
}

fn assert_reloads_only_the_pull_requests(requests: &[String]) {
    assert_only_requests_to(
        requests,
        &["git/pullrequests"],
        &["builds", "wiql", "workitemsbatch"],
    );
}

#[tokio::test]
async fn the_menu_lists_the_actions_of_an_active_pull_request() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);

    harness.press(KeyCode::Char('e'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::PrActions { .. })).await;

    let screen = harness.screen();
    for label in [
        "Abandon",
        "Mark as draft",
        "Edit title",
        "Edit description",
        "Add reviewer",
        "Remove reviewer",
        "Link work item",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(!screen.contains("Reactivate"));
}

#[tokio::test]
async fn abandoning_needs_a_confirmation_and_patches_the_status() {
    let mut harness = connected().await;
    expect_patch(&harness, PR_101, r#""status":"abandoned""#).await;
    pick_action(&mut harness, 6).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Confirm { .. })).await;
    assert!(harness.screen().contains("Abandon !101"));
    harness.settle().await;
    assert_eq!(harness.ado.received("PATCH", "/101").await, 0);
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    let requests = harness.ado.requests().await.split_off(before);
    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn abandoning_without_confirming_sends_nothing() {
    for key in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut harness = connected().await;
        pick_action(&mut harness, 6).await;
        wait_for_popup(&mut harness, |p| matches!(p, Popup::Confirm { .. })).await;
        harness.settle().await;
        let before = harness.ado.requests().await.len();

        harness.press(key);
        harness.settle().await;

        assert!(harness.app.popup.is_none());
        assert_eq!(harness.ado.requests().await.len(), before);
    }
}

#[tokio::test]
async fn reactivating_patches_the_status_back_to_active() {
    let mut harness = with_changed_pr(|pr| pr["status"] = json!("abandoned")).await;
    expect_patch(&harness, PR_101, r#""status":"active""#).await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('e'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::PrActions { .. })).await;
    assert!(harness.screen().contains("Reactivate"));
    assert!(!harness.screen().contains("Abandon"));

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
}

#[tokio::test]
async fn marking_as_draft_patches_is_draft() {
    let mut harness = connected().await;
    expect_patch(&harness, PR_101, r#""isDraft":true"#).await;
    pick_action(&mut harness, 5).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Confirm { .. })).await;
    assert!(harness.screen().contains("as draft?"));
    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
    assert!(
        harness
            .ado
            .requests()
            .await
            .contains(&format!("PATCH {PR_101}"))
    );
}

#[tokio::test]
async fn publishing_a_draft_patches_is_draft_off() {
    let mut harness = with_changed_pr(|pr| pr["isDraft"] = json!(true)).await;
    expect_patch(&harness, PR_101, r#""isDraft":false"#).await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('e'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::PrActions { .. })).await;
    assert!(harness.screen().contains("Publish"));
    assert!(!harness.screen().contains("Mark as draft"));
    harness.press(KeyCode::Enter);
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Confirm { .. })).await;
    assert!(harness.screen().contains("Publish !101"));

    harness.settle().await;
    let before = harness.ado.requests().await.len();
    harness.press(KeyCode::Char('y'));
    harness.settle().await;
    let requests = harness.ado.requests().await.split_off(before);

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
}

#[tokio::test]
async fn editing_the_title_patches_the_new_title() {
    let mut harness = connected().await;
    expect_patch(&harness, PR_101, r#""title":"Add order filter v2""#).await;
    pick_action(&mut harness, 0).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    assert!(harness.screen().contains("Add order filter"));
    harness.type_text(" v2");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn editing_the_description_starts_from_the_full_text() {
    let mut harness = connected().await;
    expect_patch(
        &harness,
        PR_101,
        r#""description":"Adds a status filter to the order list. Done.""#,
    )
    .await;
    pick_action(&mut harness, 1).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    assert!(
        harness
            .screen()
            .contains("Adds a status filter to the order list.")
    );
    harness.type_text(" Done.");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn adding_a_reviewer_looks_up_the_identity_and_puts_it() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/_apis/identities"))
                .and(query_param("filterValue", "anna@example.com")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [{ "id": "anna-id" }] })),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            Mock::given(method("PUT"))
                .and(path(REVIEWER_ANNA))
                .and(body_string_contains(r#""vote":0"#)),
            ResponseTemplate::new(200).set_body_json(json!({ "id": "anna-id", "vote": 0 })),
            1,
        )
        .await;
    pick_action(&mut harness, 2).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    harness.type_text("anna@example.com");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PUT {REVIEWER_ANNA}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn removing_a_reviewer_deletes_it() {
    let mut harness = with_changed_pr(anna_has_an_id).await;
    harness
        .ado
        .expect(
            Mock::given(method("DELETE")).and(path(REVIEWER_ANNA)),
            ResponseTemplate::new(204),
            1,
        )
        .await;
    pick_action(&mut harness, 3).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::RemoveReviewer { .. })).await;
    assert!(harness.screen().contains("Anna Reviewer"));

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("DELETE {REVIEWER_ANNA}")),
        "{requests:?}"
    );
    assert_reloads_only_the_pull_requests(&requests);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn linking_a_work_item_adds_an_artifact_link_and_reloads_only_the_pull_request() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH"))
                .and(path(WORK_ITEM_301))
                .and(body_string_contains("/relations/-"))
                .and(body_string_contains(
                    "vstfs:///Git/PullRequestId/project-fabrikam%2Frepo-api%2F101",
                )),
            ResponseTemplate::new(200).set_body_json(json!({ "id": 301 })),
            1,
        )
        .await;
    pick_action(&mut harness, 4).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    harness.type_text("#301");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {WORK_ITEM_301}")),
        "{requests:?}"
    );
    assert_only_requests_to(
        &requests,
        &["git/pullrequests", "workitems/301"],
        &["builds", "wiql"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn a_failed_change_shows_the_error_without_reloading() {
    let mut harness = connected().await;
    harness.ado.fail("PATCH", PR_101, 409).await;
    pick_action(&mut harness, 0).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    harness.type_text("!");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(harness.app.error.is_some());
    assert!(
        requests.iter().all(|r| r.starts_with("PATCH ")),
        "{requests:?}"
    );
}
