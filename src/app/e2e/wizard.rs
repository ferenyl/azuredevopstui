//! The setup wizard: going back a step, and changing the project from the running app.

use crossterm::event::KeyCode;
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

use super::{Harness, Screen, SetupStep};

fn on_step(app: &super::App, title: &str) -> bool {
    matches!(&app.screen, Screen::Setup(step) if step.title() == title)
}

fn picking(h: &Harness) -> Option<String> {
    match &h.app.screen {
        Screen::Setup(step) => step
            .selection()
            .and_then(|s| s.current())
            .map(str::to_string),
        Screen::Main => None,
    }
}

#[tokio::test]
async fn esc_goes_back_through_every_step_then_setup_finishes() {
    let mut h = Harness::unconfigured().await;
    h.connect().await;
    assert!(on_step(&h.app, "Select organization"));

    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select project")).await;
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select team")).await;
    h.press(KeyCode::Esc);
    assert!(on_step(&h.app, "Select project"));
    assert_eq!(picking(&h).as_deref(), Some("Fabrikam"));
    assert!(h.app.config.is_none());

    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select team")).await;
    assert_eq!(picking(&h).as_deref(), Some("Mobile"));
    h.press(KeyCode::Down);
    assert_eq!(picking(&h).as_deref(), Some("Web"));
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select ready column")).await;
    assert!(h.app.config.is_none(), "nothing is saved before the column");

    h.press(KeyCode::Esc);
    assert!(on_step(&h.app, "Select team"));
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select ready column")).await;
    h.press(KeyCode::Down);
    assert_eq!(picking(&h).as_deref(), Some("Ready"));
    h.press(KeyCode::Enter);
    h.wait_for(h_is_main).await;
    h.settle().await;

    let config = h.config();
    assert_eq!(
        (config.project.as_str(), config.team.as_str()),
        ("Fabrikam", "Web")
    );
    assert_eq!(config.ready_column.as_deref(), Some("Ready"));
    assert!(h.screen().contains("Inbox"));
}

#[tokio::test]
async fn esc_on_the_first_step_quits() {
    let mut h = Harness::unconfigured().await;
    h.connect().await;

    h.press(KeyCode::Esc);

    assert!(h.app.should_quit);
}

fn h_is_main(app: &super::App) -> bool {
    matches!(app.screen, Screen::Main)
}

/// Answers everything under the Mobile team with an empty board.
async fn mount_mobile(h: &Harness) {
    h.ado
        .expect(
            Mock::given(method("GET")).and(path_regex(r"^/contoso/Fabrikam/Mobile/")),
            ResponseTemplate::new(200).set_body_json(json!({
                "value": [{ "name": "Stories", "type": "requirement" }],
                "workItems": [],
                "columns": [{ "name": "New" }, { "name": "Ready" }],
                "fields": {
                    "columnField": { "referenceName": "Column" },
                    "doneField": { "referenceName": "Column.Done" }
                }
            })),
            1..,
        )
        .await;
    h.ado
        .expect(
            Mock::given(method("POST")).and(path_regex(r"^/contoso/Fabrikam/Mobile/")),
            ResponseTemplate::new(200).set_body_json(json!({ "workItems": [], "value": [] })),
            0..,
        )
        .await;
}

#[tokio::test]
async fn p_switches_team_without_a_restart() {
    let mut h = Harness::new().await;
    mount_mobile(&h).await;
    h.connect().await;
    assert!(h.is_main());
    assert!(h.screen().contains("PR 101") || h.screen().contains("Inbox"));

    h.press(KeyCode::Char('P'));
    h.wait_for(|app| on_step(app, "Select organization")).await;
    assert_eq!(picking(&h).as_deref(), Some("contoso"));
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select project")).await;
    assert_eq!(picking(&h).as_deref(), Some("Fabrikam"));
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select team")).await;
    assert_eq!(picking(&h).as_deref(), Some("Web"));
    h.press(KeyCode::Up);
    assert_eq!(picking(&h).as_deref(), Some("Mobile"));
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select ready column")).await;
    assert_eq!(h.config().team, "Web", "still on the old team");
    h.press(KeyCode::Enter);
    h.wait_for(h_is_main).await;
    h.settle().await;

    let config = h.config();
    assert_eq!(config.team, "Mobile");
    assert_eq!(config.ready_column.as_deref(), Some("New"));
    assert!(!h.screen().contains("PR 101"));
    let requests = h.ado.requests().await;
    assert!(
        requests
            .iter()
            .any(|r| r.contains("/Fabrikam/Mobile/_apis/wit/wiql")),
        "{requests:?}"
    );
}

#[tokio::test]
async fn cancelling_a_switch_keeps_everything() {
    let mut h = Harness::new().await;
    h.connect().await;
    let before = h.screen();

    h.press(KeyCode::Char('P'));
    h.wait_for(|app| on_step(app, "Select organization")).await;
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select project")).await;
    h.press(KeyCode::Esc);
    assert!(on_step(&h.app, "Select organization"));
    h.press(KeyCode::Esc);

    assert!(h.is_main());
    assert!(!matches!(
        h.app.screen,
        Screen::Setup(SetupStep::Loading(_))
    ));
    assert_eq!(h.config().team, "Web");
    assert_eq!(h.screen(), before);
}
