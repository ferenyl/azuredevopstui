//! `b` shows the sprint's items as board lanes in the detail panel.

use crossterm::event::KeyCode;
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::connected;

const WIQL: &str = "/contoso/Fabrikam/Web/_apis/wit/wiql";
const BOARD_QUERY: &str = "BacklogPriority";

/// Counts the board queries, which the fixtures answer like any other WIQL.
async fn count_board_queries(harness: &Harness, times: u64) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(WIQL))
                .and(body_string_contains(BOARD_QUERY)),
            ResponseTemplate::new(200).set_body_json(
                json!({ "workItems": [{ "id": 301 }, { "id": 302 }, { "id": 303 }] }),
            ),
            times,
        )
        .await;
}

/// Column of the last `text` on the line, in characters.
fn column(line: &str, text: &str) -> usize {
    let at = line
        .rfind(text)
        .unwrap_or_else(|| panic!("{text} in {line}"));
    line[..at].chars().count()
}

fn line_with<'a>(screen: &'a str, text: &str) -> &'a str {
    screen
        .lines()
        .find(|line| line.contains(text))
        .unwrap_or_else(|| panic!("{text} not on screen:\n{screen}"))
}

#[tokio::test]
async fn board_shows_the_sprint_items_as_lanes() {
    let mut harness = Harness::new().await;
    count_board_queries(&harness, 1).await;
    harness.connect().await;

    harness.press(KeyCode::Char('b'));
    harness.wait_for_text("Closed (1)").await;

    let screen = harness.screen();
    for title in [
        "Sprint board",
        "New (1)",
        "Ready (0)",
        "Active (1)",
        "Closed (1)",
    ] {
        assert!(screen.contains(title), "{title}\n{screen}");
    }
    assert!(screen.contains("Sprint 41 · 3 items"), "{screen}");
    let titles = line_with(&screen, "Closed (1)");
    let (new, active, closed) = (
        column(titles, "New (1)"),
        column(titles, "Active (1)"),
        column(titles, "Closed (1)"),
    );
    // The card starts one column after the lane's border, the title two.
    let card = |id: &str| column(line_with_card(&screen, id), id);
    assert!(card("#302") + 2 > new && card("#302") < active, "{screen}");
    assert!(
        card("#301") + 2 > active && card("#301") < closed,
        "{screen}"
    );
    assert!(card("#303") + 2 > closed, "{screen}");
    assert!(line_with_card(&screen, "#301").contains("MM"), "{screen}");
    assert!(line_with_card(&screen, "#302").contains("OO"), "{screen}");
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

/// The line of the board card `id`; the lists on the left show the same ids.
fn line_with_card<'a>(screen: &'a str, id: &str) -> &'a str {
    screen
        .lines()
        .filter(|line| line.matches(id).count() > 0)
        .max_by_key(|line| column(line, id))
        .unwrap_or_else(|| panic!("{id} not on screen:\n{screen}"))
}

#[tokio::test]
async fn reload_refreshes_the_board_and_hiding_it_stops_that() {
    let mut harness = Harness::new().await;
    count_board_queries(&harness, 2).await;
    harness.connect().await;

    harness.press(KeyCode::Char('b'));
    harness.wait_for_text("Closed (1)").await;
    harness.press(KeyCode::Char('r'));
    harness.settle().await;
    harness.press(KeyCode::Char('b'));
    harness.press(KeyCode::Char('r'));
    harness.settle().await;

    let screen = harness.screen();
    assert!(screen.contains("My items per column"), "{screen}");
    assert!(!screen.contains("Closed (1)"), "{screen}");
}

#[tokio::test]
async fn esc_and_opening_an_item_leave_the_board() {
    let mut harness = connected().await;

    harness.press(KeyCode::Char('b'));
    harness.press(KeyCode::Esc);
    assert!(!harness.app.board_view);
    assert!(harness.screen().contains("My items per column"));

    harness.press(KeyCode::Char('b'));
    harness.press(KeyCode::Enter);
    assert!(!harness.app.board_view);
    assert!(harness.app.detail.is_some());
}

#[tokio::test]
async fn board_lists_the_columns_on_a_narrow_terminal() {
    let mut harness = connected().await;

    harness.press(KeyCode::Char('b'));
    harness.wait_for(|app| app.data.board_items.is_some()).await;

    let screen = harness.screen_at(80, 24);
    for heading in ["New (1)", "Ready (0)", "Active (1)", "Closed (1)"] {
        assert!(screen.contains(heading), "{heading}\n{screen}");
    }
    assert!(screen.contains("#302 Crash on empty"), "{screen}");
}

#[tokio::test]
async fn failing_board_query_shows_the_error_and_keeps_loading() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(WIQL))
                .and(body_string_contains(BOARD_QUERY)),
            ResponseTemplate::new(500).set_body_json(json!({ "message": "mock failure" })),
            1..,
        )
        .await;
    harness.connect().await;

    harness.press(KeyCode::Char('b'));
    harness.wait_for(|app| app.error.is_some()).await;

    assert!(harness.screen().contains("Loading…"));
}
