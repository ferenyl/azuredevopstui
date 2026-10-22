use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    MY_PRS, PR_101, assert_only_requests_to, connected, fixture_pull_requests, select,
    wait_for_popup,
};
use crate::app::{Panel, Popup};

const PR_202_REVIEWER: &str =
    "/contoso/project-fabrikam/_apis/git/repositories/repo-api/pullRequests/202/reviewers/me-id";

fn reviewer(vote: i32) -> Value {
    json!({ "id": "me-id", "displayName": "Mona Me", "vote": vote, "isRequired": true })
}

/// Everyone's PRs with my vote on PR 202.
fn others_with_vote_on_202(vote: i32) -> Value {
    let mut prs = fixture_pull_requests("pull-requests-all.json");
    for pr in &mut prs {
        if pr["pullRequestId"] == 202 {
            pr["reviewers"] = json!([reviewer(vote)]);
        }
    }
    json!({ "value": prs })
}

fn my_pr_with_vote(vote: i32) -> Value {
    let mut prs = fixture_pull_requests("pull-requests-mine.json");
    prs[0]["reviewers"] = json!([reviewer(vote)]);
    json!({ "value": prs })
}

async fn answer_my_prs_with_vote(harness: &Harness, vote: i32) {
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(my_pr_with_vote(vote)),
            1,
        )
        .await;
}

async fn expect_vote(harness: &Harness, target: &str, vote: i32) {
    harness
        .ado
        .expect(
            Mock::given(method("PUT"))
                .and(path(target))
                .and(body_string_contains(format!(r#""vote":{vote}"#))),
            ResponseTemplate::new(200).set_body_json(json!({ "id": "me-id", "vote": vote })),
            1,
        )
        .await;
}

fn inbox_keys(harness: &Harness) -> Vec<String> {
    harness
        .app
        .data
        .inbox()
        .iter()
        .map(|entry| entry.detail.key())
        .collect()
}

#[tokio::test]
async fn all_five_votes_are_sent_and_shown_in_the_detail() {
    let votes = [
        (10, 0, "approved"),
        (5, 1, "approved with suggestions"),
        (-5, 2, "waiting for author"),
        (-10, 3, "rejected"),
        (0, 4, "no vote"),
    ];
    for (vote, downs, label) in votes {
        let mut harness = Harness::new().await;
        expect_vote(&harness, &format!("{PR_101}/reviewers/me-id"), vote).await;
        harness.connect().await;
        answer_my_prs_with_vote(&harness, vote).await;
        select(&mut harness, Panel::MyPrs, 101);
        harness.press(KeyCode::Enter);
        harness.press(KeyCode::Char('v'));
        wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
        assert!(harness.screen().contains("Approve with suggestions"));
        for _ in 0..downs {
            harness.press(KeyCode::Char('j'));
        }

        harness.press(KeyCode::Enter);
        harness.settle().await;

        assert!(
            harness.app.error.is_none(),
            "{vote}: {:?}",
            harness.app.error
        );
        assert!(
            harness.screen().contains(label),
            "{vote}:\n{}",
            harness.screen()
        );
    }
}

#[tokio::test]
async fn voting_drops_the_pull_request_from_the_inbox() {
    let mut harness = Harness::new().await;
    for (vote, times) in [(0, 1), (10, 1)] {
        harness
            .ado
            .expect_first(
                Mock::given(method("GET")).and(path(MY_PRS)).and(
                    wiremock::matchers::query_param_is_missing("searchCriteria.creatorId"),
                ),
                ResponseTemplate::new(200).set_body_json(others_with_vote_on_202(vote)),
                times,
            )
            .await;
    }
    expect_vote(&harness, PR_202_REVIEWER, 10).await;
    harness.connect().await;
    assert!(inbox_keys(&harness).contains(&"!202".to_string()));
    let row = inbox_keys(&harness)
        .iter()
        .position(|key| key == "!202")
        .unwrap();
    harness.app.focus = Panel::Inbox;
    harness.app.selections[0] = row;
    harness.press(KeyCode::Char('v'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
    for _ in 0..4 {
        harness.press(KeyCode::Char('k'));
    }
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Enter);
    harness.settle().await;

    let requests = harness.ado.requests().await.split_off(before);
    assert!(
        requests.contains(&format!("PUT {PR_202_REVIEWER}")),
        "{requests:?}"
    );
    assert_only_requests_to(
        &requests,
        &["git/pullrequests"],
        &["builds", "wiql", "workitemsbatch"],
    );
    assert!(!inbox_keys(&harness).contains(&"!202".to_string()));
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn failed_vote_shows_the_error_without_reloading() {
    let mut harness = connected().await;
    harness
        .ado
        .fail("PUT", &format!("{PR_101}/reviewers/me-id"), 403)
        .await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('v'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
    harness.settle().await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Enter);
    harness.settle().await;

    let requests = harness.ado.requests().await.split_off(before);
    assert!(harness.app.error.is_some());
    assert!(
        requests.iter().all(|r| r.starts_with("PUT ")),
        "{requests:?}"
    );
}

#[tokio::test]
async fn escaping_the_vote_popup_sends_nothing() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Char('v'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
    harness.settle().await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Esc);
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert_eq!(harness.ado.requests().await.len(), before);
}
