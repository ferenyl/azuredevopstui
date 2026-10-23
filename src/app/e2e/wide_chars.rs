use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{MY_PRS, PR_101, fixture_pull_requests, select};
use crate::app::{DetailInfo, Panel};

const TITLE: &str = "修复登录 🚀";
const REVIEWER: &str = "李雷 Reviewer";
const DESCRIPTION: &str = "修复登录页面的错误修复登录页面的错误修复登录页面的错误修复登录页面的错误 \
    🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀 and some ASCII words to wrap";

/// PR 101 with a CJK/emoji title, description and reviewer.
async fn wide_pull_request() -> Harness {
    let mut harness = Harness::new().await;
    let mut mine = fixture_pull_requests("pull-requests-mine.json");
    mine[0]["title"] = TITLE.into();
    mine[0]["reviewers"][0]["displayName"] = REVIEWER.into();
    harness
        .ado
        .expect_first(
            Mock::given(method("GET"))
                .and(path(MY_PRS))
                .and(query_param("searchCriteria.creatorId", "me-id")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": mine })),
            1,
        )
        .await;
    let mut body: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/pr-101.json", super::FIXTURES)).unwrap(),
    )
    .unwrap();
    let mut pr = body["body"].take();
    pr["title"] = TITLE.into();
    pr["description"] = DESCRIPTION.into();
    pr["reviewers"][0]["displayName"] = REVIEWER.into();
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(PR_101)),
            ResponseTemplate::new(200).set_body_json(pr),
            0..,
        )
        .await;
    harness.connect().await;
    harness
}

/// Screen rows as cells: the cell after a wide glyph is blank, so every row has one symbol per column.
fn rows(harness: &Harness) -> Vec<Vec<char>> {
    harness
        .screen()
        .lines()
        .map(|row| row.chars().collect())
        .collect()
}

/// The row without the blank cells that follow wide glyphs and without spaces.
fn compact(row: &[char]) -> String {
    row.iter().filter(|c| !c.is_whitespace()).collect()
}

#[tokio::test]
async fn wide_titles_in_the_list_keep_the_panel_border_aligned() {
    let harness = wide_pull_request().await;

    let rows = rows(&harness);
    let ascii = rows
        .iter()
        .find(|row| compact(row).contains("Fixloginredirect"))
        .expect("ascii row");
    let border = ascii.iter().position(|c| *c == '│').expect("border");
    let border = ascii[border + 1..]
        .iter()
        .position(|c| *c == '│')
        .expect("panel border")
        + border
        + 1;
    let wide = rows
        .iter()
        .find(|row| compact(row).contains("修复登录🚀"))
        .expect("wide row");

    assert_eq!(wide.len(), ascii.len());
    assert_eq!(wide[border], '│');
}

#[tokio::test]
async fn wide_description_wraps_inside_the_detail_panel() {
    let mut harness = wide_pull_request().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.wait_for_text("ASCII").await;

    let rows = rows(&harness);
    let last = rows[0].len() - 1;
    assert_eq!(last, 159);
    let detail_rows: Vec<_> = rows
        .iter()
        .filter(|row| {
            let text = compact(row);
            text.contains("修复登录页面") || text.contains("🚀🚀") || text.contains("李雷Reviewer")
        })
        .collect();
    assert!(detail_rows.len() >= 3, "{detail_rows:?}");
    for row in detail_rows {
        assert_eq!(row[last], '│', "{}", row.iter().collect::<String>());
    }
}
