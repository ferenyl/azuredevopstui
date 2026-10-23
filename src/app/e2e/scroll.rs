use crossterm::event::KeyCode;
use wiremock::ResponseTemplate;
use wiremock::matchers::{method, path};

use super::Harness;
use super::flows::{PR_101, select};
use crate::app::{DetailInfo, Panel};

/// Opens PR 101 with a 60-line description and focuses the detail.
async fn pull_request_with_long_description() -> Harness {
    let mut harness = Harness::new().await;
    let mut body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/pr-101.json", super::FIXTURES)).unwrap(),
    )
    .unwrap();
    let mut pr = body["body"].take();
    pr["description"] = (1..=60)
        .map(|n| format!("Line {n}"))
        .collect::<Vec<_>>()
        .join("\n\n")
        .into();
    harness
        .ado
        .expect(
            wiremock::Mock::given(method("GET")).and(path(PR_101)),
            ResponseTemplate::new(200).set_body_json(pr),
            1..,
        )
        .await;
    harness.connect().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.wait_for_text("Line 1").await;
    harness
}

#[tokio::test]
async fn scrolling_down_stops_at_the_end_of_the_content() {
    let mut harness = pull_request_with_long_description().await;
    harness.app.focus = Panel::Detail;

    for _ in 0..200 {
        harness.press(KeyCode::Char('j'));
    }
    harness.wait_for_text("Line 60").await;

    let max = harness.app.detail_max_scroll.get();
    assert!(max > 0);
    assert_eq!(harness.app.detail_scroll, max);

    harness.press(KeyCode::Char('k'));
    assert_eq!(harness.app.detail_scroll, max - 1);
}
