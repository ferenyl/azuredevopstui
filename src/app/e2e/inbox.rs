//! Inbox row layout.

use serde_json::json;
use wiremock::matchers::{method, path, query_param_is_missing};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{MY_PRS, fixture_pull_requests};

#[tokio::test]
async fn inbox_marker_is_followed_by_a_single_space() {
    let mut harness = Harness::new().await;
    let mut prs = fixture_pull_requests("pull-requests-all.json");
    for pr in &mut prs {
        if pr["pullRequestId"] == 202 {
            pr["reviewers"] =
                json!([{ "id": "me-id", "displayName": "Mona Me", "vote": 0, "isRequired": true }]);
        }
    }
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param_is_missing("searchCriteria.creatorId")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": prs })),
            1,
        )
        .await;
    harness.connect().await;

    let screen = harness.screen();

    let row = screen
        .lines()
        .find(|line| line.contains("│   !202") || line.starts_with("│!202"))
        .unwrap_or_default();
    assert!(row.contains("◉ Fix login"), "{screen}");
    assert!(!row.contains("◉  Fix login"), "{screen}");
}
