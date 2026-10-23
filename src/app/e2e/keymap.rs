use std::sync::Arc;

use crossterm::event::KeyCode;

use super::flows::{select, wait_for_popup};
use super::{Harness, config};
use crate::app::{Keymap, Panel, Popup};
use crate::config::Config;

/// Connected with `keys` from the config, like a user's config.json.
async fn connected_with_keys(keys: serde_json::Value) -> Harness {
    let mut harness = Harness::new().await;
    let mut config = config();
    config.keys = serde_json::from_value(keys).unwrap();
    harness.app.keymap = Keymap::new(&config.keys).unwrap();
    harness.app.config = Some(Arc::new(config));
    harness.connect().await;
    harness
}

#[tokio::test]
async fn rebound_vote_key_opens_the_vote_popup_and_the_old_key_does_nothing() {
    let mut harness = connected_with_keys(serde_json::json!({ "vote": ["V"] })).await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness.settle().await;

    harness.press(KeyCode::Char('v'));
    harness.settle().await;
    assert!(harness.app.popup.is_none());
    let screen = harness.screen();
    assert!(screen.contains("[V] vote"), "{screen}");
    assert!(!screen.contains("[v] vote"), "{screen}");

    harness.press(KeyCode::Char('V'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::Vote { .. })).await;
}

#[tokio::test]
async fn help_shows_the_configured_keys() {
    let mut harness =
        connected_with_keys(serde_json::json!({ "help": ["f1"], "zoom": ["Z"] })).await;

    harness.press(KeyCode::Char('?'));
    assert!(!harness.app.show_help);
    harness.press(KeyCode::F(1));

    let screen = harness.screen();
    assert!(harness.app.show_help);
    assert!(screen.contains(" Z              zoom"), "{screen}");
    assert!(screen.contains(" f1             help"), "{screen}");
}

#[test]
fn invalid_keys_in_the_config_file_are_a_parse_error() {
    let json = r#"{"organization":"o","project":"p","team":"t","keys":{"vote":["m"]}}"#;

    let error = Config::parse(json).unwrap_err();

    assert_eq!(
        error.to_string(),
        "key \"m\" is bound to both \"comment\" and \"vote\" in keys"
    );
}
