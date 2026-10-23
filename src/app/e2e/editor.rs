use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{COMMENTS_301, connected, select};
use crate::app::{Panel, Popup};

fn newline(harness: &mut Harness) {
    harness
        .app
        .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
}

async fn comment_popup() -> Harness {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('m'));
    harness
}

#[tokio::test]
async fn insert_after_moving_up_lands_in_the_right_line() {
    let mut harness = comment_popup().await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(COMMENTS_301))
                .and(body_string_contains(r"one  \ntwoX  \nthree")),
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": 2 })),
            1,
        )
        .await;

    harness.type_text("one");
    newline(&mut harness);
    harness.type_text("two");
    newline(&mut harness);
    harness.type_text("three");
    harness.press(KeyCode::Up);
    harness.type_text("X");
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn long_text_scrolls_inside_the_screen_and_keeps_the_cursor_visible() {
    let mut harness = comment_popup().await;

    for n in 1..=60 {
        harness.type_text(&format!("line{n:02}"));
        if n < 60 {
            newline(&mut harness);
        }
    }

    let screen = harness.screen();
    assert!(screen.contains("line60▏"), "{screen}");
    assert!(!screen.contains("line01"), "{screen}");
    assert_eq!(screen.lines().count(), 45);

    for _ in 0..59 {
        harness.press(KeyCode::Up);
    }
    harness.press(KeyCode::Home);
    let screen = harness.screen();
    assert!(screen.contains("▏line01"), "{screen}");
    assert!(!screen.contains("line60"), "{screen}");
}

#[tokio::test]
async fn esc_asks_before_discarding_text() {
    let mut harness = comment_popup().await;
    let before = harness.ado.requests().await.len();

    harness.type_text("draft");
    harness.press(KeyCode::Esc);
    assert!(matches!(harness.app.popup, Some(Popup::Confirm { .. })));
    assert!(harness.screen().contains("Discard the text?"));

    harness.press(KeyCode::Char('n'));
    let Some(Popup::Text { text, .. }) = &harness.app.popup else {
        panic!("text popup not restored");
    };
    assert_eq!(text.split(), ("draft", ""));

    harness.press(KeyCode::Esc);
    harness.press(KeyCode::Char('y'));
    harness.settle().await;

    assert!(harness.app.popup.is_none());
    assert_eq!(harness.ado.received("POST", "/comments").await, 0);
    assert_eq!(harness.ado.requests().await.len(), before);
}

#[tokio::test]
async fn esc_closes_an_empty_popup_without_asking() {
    let mut harness = comment_popup().await;

    harness.press(KeyCode::Esc);

    assert!(harness.app.popup.is_none());
}
