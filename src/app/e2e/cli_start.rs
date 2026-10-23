//! Starting with `--org`/`--project`: the switch opens at the team step and is saved like `P`.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::{Harness, Screen, config};

fn on_step(app: &super::App, title: &str) -> bool {
    matches!(&app.screen, Screen::Setup(step) if step.title() == title)
}

#[tokio::test]
async fn start_in_a_project_picks_a_team_and_keeps_settings() {
    let mut h = Harness::new().await;
    let mut other = config();
    other.project = "Other".into();
    other.mouse = false;
    h.app.config = Some(Arc::new(other));
    h.app.start_in("contoso".into(), Some("Fabrikam".into()));

    h.app.continue_setup();
    h.wait_for(|app| on_step(app, "Select team")).await;
    assert!(h.screen().contains("Mobile"));
    h.press(KeyCode::Down);
    h.press(KeyCode::Enter);
    h.wait_for(|app| on_step(app, "Select ready column")).await;
    assert_eq!(
        h.config().project,
        "Other",
        "nothing is saved before the column"
    );
    h.press(KeyCode::Enter);
    h.wait_for(|app| matches!(app.screen, Screen::Main)).await;
    h.settle().await;

    let config = h.config();
    assert_eq!(
        (config.project.as_str(), config.team.as_str()),
        ("Fabrikam", "Web")
    );
    assert!(!config.mouse, "other settings are kept");
    assert!(
        h.ado
            .received("GET", "/_apis/projects/Fabrikam/teams")
            .await
            > 0
    );
}

#[tokio::test]
async fn esc_at_the_started_team_step_opens_the_configured_project() {
    let mut h = Harness::new().await;
    h.ado
        .expect(
            Mock::given(method("GET")).and(path("/contoso/_apis/projects/Other/teams")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [{ "name": "Ops" }] })),
            1,
        )
        .await;
    h.app.start_in("contoso".into(), Some("Other".into()));

    h.app.continue_setup();
    h.wait_for(|app| on_step(app, "Select team")).await;
    h.press(KeyCode::Esc);
    h.wait_for(|app| matches!(app.screen, Screen::Main)).await;
    h.settle().await;

    assert_eq!(h.config().project, "Fabrikam");
    let requests = h.ado.requests().await;
    assert!(
        requests
            .iter()
            .any(|r| r.contains("/Fabrikam/Web/_apis/wit/wiql")),
        "{requests:?}"
    );
}
