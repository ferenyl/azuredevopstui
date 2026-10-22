use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    PR_101, assert_only_requests_to, connected, requests_after_confirming, select, wait_for_popup,
};
use crate::app::{DetailInfo, DetailTab, Panel, Popup};

const PR_303: &str = "/contoso/project-other/_apis/git/repositories/repo-api/pullRequests/303";

/// Opens PR `id` of `panel` on its Comments tab.
async fn open_comments(harness: &mut Harness, panel: Panel, id: u32) {
    select(harness, panel, id);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.press(KeyCode::Tab);
    assert_eq!(harness.app.detail_tab, DetailTab::Comments);
}

async fn expect_post(harness: &Harness, target: &str, body_part: &str) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(target))
                .and(body_string_contains(body_part)),
            ResponseTemplate::new(200).set_body_json(json!({ "id": 9 })),
            1,
        )
        .await;
}

/// Thread 2 (the newest, so the first one listed) of PR 101 with `status`.
fn threads_with_status_on_2(status: &str) -> Value {
    json!({ "value": [{
        "id": 2,
        "status": status,
        "threadContext": { "filePath": "/src/paging.rs" },
        "comments": [{
            "id": 2,
            "author": { "id": "other-id", "displayName": "Olle Other" },
            "content": "Second look at paging",
            "publishedDate": "2026-10-20T11:00:00.5000000Z",
            "commentType": "text"
        }]
    }] })
}

#[tokio::test]
async fn thread_status_is_shown_in_the_list() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(format!("{PR_101}/threads"))),
            ResponseTemplate::new(200).set_body_json(threads_with_status_on_2("wontFix")),
            1..,
        )
        .await;

    open_comments(&mut harness, Panel::MyPrs, 101).await;

    let screen = harness.screen();
    assert!(screen.contains("won't fix"), "{screen}");
    assert!(screen.contains("Second look at paging"), "{screen}");
}

#[tokio::test]
async fn replying_posts_under_the_selected_thread_and_reloads_only_its_detail() {
    let mut harness = connected().await;
    expect_post(
        &harness,
        &format!("{PR_101}/threads/2/comments"),
        r#""parentCommentId":2"#,
    )
    .await;
    open_comments(&mut harness, Panel::MyPrs, 101).await;
    harness.press(KeyCode::Char(']'));
    harness.press(KeyCode::Char('m'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    assert!(harness.screen().contains("Reply on !101"));
    harness.type_text("Will do");

    let requests = requests_after_confirming(&mut harness).await;

    assert_eq!(
        requests
            .iter()
            .filter(|r| r.starts_with("POST ") && r.contains("/git/"))
            .collect::<Vec<_>>(),
        [&format!("POST {PR_101}/threads/2/comments")]
    );
    assert_only_requests_to(
        &requests,
        &["/threads", "/statuses"],
        &["git/pullrequests", "builds", "wiql", "iterations"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn m_without_a_selected_thread_starts_a_new_thread() {
    let mut harness = connected().await;
    expect_post(
        &harness,
        &format!("{PR_101}/threads"),
        r#""status":"active""#,
    )
    .await;
    open_comments(&mut harness, Panel::MyPrs, 101).await;
    harness.press(KeyCode::Char('m'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    assert!(harness.screen().contains("New thread on !101"));
    harness.type_text("Heads up");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("POST {PR_101}/threads")),
        "{requests:?}"
    );
    assert_only_requests_to(
        &requests,
        &["/threads"],
        &["git/pullrequests", "builds", "wiql"],
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn new_thread_on_someone_elses_pull_request_reloads_the_other_pull_requests() {
    let mut harness = connected().await;
    expect_post(&harness, &format!("{PR_303}/threads"), r#""content":"Nit""#).await;
    open_comments(&mut harness, Panel::OtherPrs, 303).await;
    harness.press(KeyCode::Char('m'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    harness.type_text("Nit");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("POST {PR_303}/threads")),
        "{requests:?}"
    );
    assert_only_requests_to(&requests, &["git/pullrequests"], &["builds", "wiql"]);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn resolving_a_thread_patches_its_status_and_shows_it_resolved() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("PATCH"))
                .and(path(format!("{PR_101}/threads/2")))
                .and(body_string_contains(r#""status":"fixed""#)),
            ResponseTemplate::new(200).set_body_json(json!({ "id": 2 })),
            1,
        )
        .await;
    open_comments(&mut harness, Panel::MyPrs, 101).await;
    harness.press(KeyCode::Char(']'));
    harness.press(KeyCode::Char('x'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::ThreadStatus { .. })).await;
    assert!(harness.screen().contains("won't fix"));
    harness.press(KeyCode::Char('j'));
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(format!("{PR_101}/threads"))),
            ResponseTemplate::new(200).set_body_json(threads_with_status_on_2("fixed")),
            1..,
        )
        .await;

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("PATCH {PR_101}/threads/2")),
        "{requests:?}"
    );
    assert_only_requests_to(
        &requests,
        &["/threads"],
        &["git/pullrequests", "builds", "wiql"],
    );
    let screen = harness.screen();
    assert!(screen.contains("resolved"), "{screen}");
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn failed_reply_shows_the_error_without_reloading() {
    let mut harness = connected().await;
    harness
        .ado
        .fail("POST", &format!("{PR_101}/threads/2/comments"), 500)
        .await;
    open_comments(&mut harness, Panel::MyPrs, 101).await;
    harness.press(KeyCode::Char(']'));
    harness.press(KeyCode::Char('m'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Text { .. })).await;
    harness.type_text("Will do");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(harness.app.error.is_some());
    assert!(
        requests.iter().all(|r| r.starts_with("POST ")),
        "{requests:?}"
    );
}

#[tokio::test]
async fn escaping_the_status_popup_sends_nothing() {
    let mut harness = connected().await;
    open_comments(&mut harness, Panel::MyPrs, 101).await;
    harness.press(KeyCode::Char(']'));
    harness.press(KeyCode::Char('x'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::ThreadStatus { .. })).await;
    harness.settle().await;
    let before = harness.ado.requests().await.len();

    harness.press(KeyCode::Esc);
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert_eq!(harness.ado.requests().await.len(), before);
}
