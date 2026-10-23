//! `/` search in the lists and in the setup pickers.

use crossterm::event::KeyCode;

use super::flows::connected;
use super::{Harness, Screen};
use crate::app::Panel;

#[tokio::test]
async fn searching_others_prs_narrows_the_list_and_enter_opens_the_match() {
    let mut harness = connected().await;
    harness.press(KeyCode::Char('4'));
    assert!(harness.app.data.len(Panel::OtherPrs) > 1);

    harness.press(KeyCode::Char('/'));
    harness.type_text("CSV");
    assert_eq!(harness.app.data.len(Panel::OtherPrs), 1);
    let screen = harness.screen();
    assert!(screen.contains("Others' PRs (1) /CSV▏"), "{screen}");
    assert!(!screen.contains("Fix login redirect"));

    harness.press(KeyCode::Enter);
    assert!(harness.app.search.is_none());
    assert!(harness.screen().contains("Others' PRs (1) /CSV "));
    harness.press(KeyCode::Enter);

    assert!(
        harness
            .app
            .detail
            .as_ref()
            .is_some_and(|detail| detail.is_pull_request(303))
    );
}

#[tokio::test]
async fn esc_clears_the_search_and_lists_everything_again() {
    let mut harness = connected().await;
    harness.press(KeyCode::Char('4'));
    let all = harness.app.data.len(Panel::OtherPrs);

    harness.press(KeyCode::Char('/'));
    harness.type_text("zzz");
    assert_eq!(harness.app.data.len(Panel::OtherPrs), 0);
    harness.press(KeyCode::Esc);

    assert_eq!(harness.app.data.len(Panel::OtherPrs), all);
    assert!(harness.app.search.is_none());
    assert!(!harness.screen().contains("/zzz"));
}

#[tokio::test]
async fn setup_project_search_filters_and_enter_loads_the_teams_of_the_match() {
    let mut harness = Harness::unconfigured().await;
    harness.connect().await;
    harness.press(KeyCode::Enter);
    harness.wait_for(|app| step_is(app, "Select project")).await;
    assert!(harness.screen().contains("Northwind"));

    harness.press(KeyCode::Char('/'));
    harness.type_text("fab");
    let screen = harness.screen();
    assert!(screen.contains("/fab▏"), "{screen}");
    assert!(screen.contains("Fabrikam"));
    assert!(!screen.contains("Northwind"));
    harness.press(KeyCode::Enter);

    harness.wait_for(|app| step_is(app, "Select team")).await;
    assert!(harness.screen().contains("Web"));
}

#[tokio::test]
async fn esc_in_a_setup_search_clears_it_before_going_back() {
    let mut harness = Harness::unconfigured().await;
    harness.connect().await;
    harness.press(KeyCode::Enter);
    harness.wait_for(|app| step_is(app, "Select project")).await;
    harness.press(KeyCode::Char('/'));
    harness.type_text("north");

    harness.press(KeyCode::Esc);
    assert!(step_is(&harness.app, "Select project"));
    let screen = harness.screen();
    assert!(screen.contains("Fabrikam") && screen.contains("Northwind"));
    assert!(!screen.contains("/north"));

    harness.press(KeyCode::Esc);
    assert!(step_is(&harness.app, "Select organization"));
}

fn step_is(app: &super::App, title: &str) -> bool {
    matches!(&app.screen, Screen::Setup(step) if step.title() == title)
}
