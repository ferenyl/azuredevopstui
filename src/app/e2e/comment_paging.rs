use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{COMMENTS_301, select};
use crate::app::{DetailInfo, DetailTab, Panel};

fn first_page() -> Value {
    let comments: Vec<Value> = (1..=10)
        .map(|n| {
            json!({
                "createdBy": { "id": "u1", "displayName": "Anna" },
                "createdDate": format!("2026-10-{n:02}T10:00:00Z"),
                "text": format!("<div>Newer comment {n}</div>"),
            })
        })
        .collect();
    json!({ "totalCount": 12, "continuationToken": "page2", "comments": comments })
}

async fn paged_comments() -> Harness {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(COMMENTS_301))
                .and(query_param("$top", "10")),
            ResponseTemplate::new(200).set_body_json(first_page()),
            1..,
        )
        .await;
    harness.connect().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::WorkItem(_))))
        .await;
    harness.app.select_tab(DetailTab::Comments);
    harness.app.focus = Panel::Detail;
    harness.wait_for_text("Newer comment 1").await;
    harness
}

async fn comment_requests(harness: &Harness) -> usize {
    harness.ado.received("GET", "/workItems/301/comments").await
}

#[tokio::test]
async fn scrolling_to_the_end_loads_older_comments_once() {
    let mut harness = paged_comments().await;
    let screen = harness.screen();
    assert!(screen.contains("scroll down for older"), "{screen}");
    assert!(!screen.contains("Older comment A"), "{screen}");
    let before = comment_requests(&harness).await;

    harness.press(KeyCode::Char('G'));
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));
    harness.wait_for_text("Older comment B").await;

    let screen = harness.screen();
    assert!(screen.contains("Older comment A"), "{screen}");
    assert!(!screen.contains("scroll down for older"), "{screen}");
    harness.press(KeyCode::Char('G'));
    harness.press(KeyCode::Char('j'));
    harness.settle().await;
    assert_eq!(comment_requests(&harness).await, before + 1);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn failing_older_page_shows_the_error_and_keeps_the_first_page() {
    let mut harness = paged_comments().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path(COMMENTS_301))
                .and(query_param("continuationToken", "page2")),
            ResponseTemplate::new(400).set_body_json(json!({ "message": "mock failure" })),
            1..,
        )
        .await;

    harness.press(KeyCode::Char('G'));
    harness.wait_for(|app| app.error.is_some()).await;

    let screen = harness.screen();
    assert!(screen.contains("Newer comment 1"), "{screen}");
    assert!(!screen.contains("Older comment A"), "{screen}");
    assert!(screen.contains("scroll down for older"), "{screen}");
}
