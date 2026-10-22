use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Harness;
use super::flows::{connected, select};
use crate::app::{DetailInfo, DetailTab, Panel};

const PR_101: &str = "/contoso/project-fabrikam/_apis/git/repositories/repo-api/pullRequests/101";
const ITEMS: &str = "/contoso/project-fabrikam/_apis/git/repositories/repo-api/items";

/// Opens my PR 101 and waits for its details.
async fn open_pull_request() -> Harness {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness
}

/// Tabs to the Files tab, focuses the detail and waits for the file list.
async fn show_files(harness: &mut Harness) {
    while harness.app.detail_tab != DetailTab::Files {
        harness.press(KeyCode::Tab);
    }
    harness
        .app
        .handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    harness.wait_for_text("M /src/orders/filter.rs").await;
}

async fn requests_to(harness: &Harness, parts: &[&str]) -> Vec<String> {
    harness
        .ado
        .requests()
        .await
        .into_iter()
        .filter(|request| parts.iter().any(|part| request.contains(part)))
        .collect()
}

#[tokio::test]
async fn files_tab_lists_the_changed_files_of_the_latest_iteration() {
    let mut harness = open_pull_request().await;

    show_files(&mut harness).await;

    let screen = harness.screen();
    assert!(screen.contains("Files (2)"), "{screen}");
    assert!(screen.contains("A /docs/filter.md"), "{screen}");
    assert!(!screen.contains("/src/orders\n"), "{screen}");
    assert_eq!(
        requests_to(&harness, &["/pullRequests/101/iterations"]).await,
        [
            format!("GET {PR_101}/iterations"),
            format!("GET {PR_101}/iterations/2/changes"),
        ]
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn enter_shows_the_diff_of_the_selected_file_and_esc_goes_back() {
    let mut harness = open_pull_request().await;
    show_files(&mut harness).await;

    harness.press(KeyCode::Enter);
    harness
        .wait_for_text("+pub fn filter(orders: &[Order], status: Status) -> Vec<Order> {")
        .await;

    let screen = harness.screen();
    assert!(screen.contains("▍/src/orders/filter.rs"), "{screen}");
    assert!(screen.contains("@@ -1,3 +1,3 @@"), "{screen}");
    assert!(
        screen.contains("-pub fn filter(orders: &[Order]) -> Vec<Order> {"),
        "{screen}"
    );
    assert!(screen.contains(" }"), "{screen}");
    harness.press(KeyCode::Esc);
    harness.wait_for_text("A /docs/filter.md").await;
    assert!(harness.app.detail.is_some());
}

#[tokio::test]
async fn added_file_is_read_only_at_the_source_commit() {
    let mut harness = open_pull_request().await;
    show_files(&mut harness).await;

    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    harness.wait_for_text("+# Order filter").await;

    assert_eq!(harness.ado.received("GET", ITEMS).await, 1);
    assert!(harness.screen().contains("+Filter orders by status."));
}

#[tokio::test]
async fn changes_load_only_when_the_files_tab_is_shown() {
    let mut harness = open_pull_request().await;
    harness.settle().await;
    assert!(
        requests_to(&harness, &["/pullRequests/101/iterations", "/items"])
            .await
            .is_empty()
    );

    show_files(&mut harness).await;
    harness.press(KeyCode::Enter);
    harness.wait_for_text("@@ -1,3 +1,3 @@").await;
    let before = harness.ado.requests().await.len();
    harness.app.refresh();
    harness.settle().await;

    let refreshed = harness.ado.requests().await.split_off(before);
    assert!(
        !refreshed
            .iter()
            .any(|request| request.contains("/pullRequests/101/iterations")
                || request.contains("/items")),
        "{refreshed:#?}"
    );
    assert!(
        refreshed
            .iter()
            .any(|request| request.ends_with("/pullRequests/101"))
    );
    assert!(harness.screen().contains("@@ -1,3 +1,3 @@"));
}
