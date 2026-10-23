//! Panel layout on small terminals and zoom.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::flows::{connected, select};
use crate::app::{DetailInfo, Panel};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

#[tokio::test]
async fn small_terminal_expands_the_focused_panel_and_keeps_every_title() {
    let mut harness = connected().await;
    assert_eq!(harness.app.focus, Panel::Inbox);

    let screen = harness.screen_at(WIDTH, HEIGHT);

    for title in [
        "Inbox",
        "My PRs",
        "My work items",
        "Others' PRs",
        "Ready",
        "My pipelines",
    ] {
        assert!(screen.contains(title), "{title}\n{screen}");
    }
    assert!(screen.contains("Push after my vote"), "{screen}");

    harness
        .app
        .handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert_eq!(harness.app.focus, Panel::MyPrs);

    let screen = harness.screen_at(WIDTH, HEIGHT);
    assert!(screen.contains("│!101  ✎2  Add order filter"), "{screen}");
    assert!(!screen.contains("Push after my vote"), "{screen}");
}

#[tokio::test]
async fn zoomed_detail_uses_the_full_width() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.app.focus = Panel::Detail;

    harness.press(KeyCode::Char('z'));
    let screen = harness.screen_at(WIDTH, HEIGHT);

    assert!(!screen.contains("My pipelines"), "{screen}");
    assert!(screen.starts_with("┌ !101 Add order filter"), "{screen}");
    let description = screen
        .lines()
        .find(|line| line.contains("Adds a status filter"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(description.starts_with("│   Adds"), "{screen}");
    assert!(description.ends_with('│'), "{screen}");

    harness.press(KeyCode::Char('z'));
    let screen = harness.screen_at(WIDTH, HEIGHT);
    assert!(screen.contains("My pipelines"), "{screen}");
    assert!(screen.contains("Adds a status filter"), "{screen}");
}
