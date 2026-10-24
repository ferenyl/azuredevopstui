use std::sync::Arc;

use crossterm::event::KeyCode;
use wiremock::ResponseTemplate;
use wiremock::matchers::{method, path};

use super::Harness;
use super::flows::{PR_101, select};
use crate::app::{DetailInfo, Panel};

const DOCS: &str = "https://example.com/docs";
const SPEC: &str = "https://example.com/spec";

/// Opens PR 101 with two links in its description.
async fn pull_request_with_links() -> Harness {
    let mut harness = Harness::new().await;
    let mut body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/pr-101.json", super::FIXTURES)).unwrap(),
    )
    .unwrap();
    let mut pr = body["body"].take();
    pr["description"] = format!("See [the docs]({DOCS}) and [the spec]({SPEC}).").into();
    harness
        .ado
        .expect(
            wiremock::Mock::given(method("GET")).and(path(PR_101)),
            ResponseTemplate::new(200).set_body_json(pr),
            1..,
        )
        .await;
    harness.connect().await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.wait_for_text("the spec").await;
    harness
}

#[tokio::test]
async fn l_selects_the_links_of_the_description_in_turn() {
    let mut harness = pull_request_with_links().await;
    assert_eq!(harness.app.link_index, None);

    harness.press(KeyCode::Char('l'));
    harness.press(KeyCode::Char('l'));
    harness.wait_for(|app| app.link_index == Some(1)).await;
    harness.press(KeyCode::Char('l'));
    harness.wait_for(|app| app.link_index == Some(0)).await;
    harness.press(KeyCode::Char('L'));
    harness.wait_for(|app| app.link_index == Some(1)).await;

    assert_eq!(*harness.app.detail_links.borrow(), vec![DOCS, SPEC]);
}

#[tokio::test]
async fn o_opens_the_selected_link_and_y_copies_it() {
    let mut harness = pull_request_with_links().await;
    let mut config = (*harness.config()).clone();
    config.browser_command = Some("rustc --version".into());
    harness.app.config = Some(Arc::new(config));

    harness.press(KeyCode::Char('l'));
    harness.press(KeyCode::Char('o'));
    harness.press(KeyCode::Char('Y'));
    harness
        .wait_for(|app| {
            app.notice.as_ref().is_some_and(|(text, _)| {
                text == &format!("sent {DOCS} to the terminal clipboard (OSC 52)")
            })
        })
        .await;

    assert_eq!(harness.app.opened, [DOCS]);
    assert_eq!(harness.app.error, None);
}

#[tokio::test]
async fn toolbar_offers_open_link_while_a_link_is_selected() {
    let mut harness = pull_request_with_links().await;
    assert!(!harness.screen().contains("open link"));

    harness.press(KeyCode::Char('l'));

    harness.wait_for_text("open link").await;
}

#[tokio::test]
async fn switching_tab_clears_the_selected_link() {
    let mut harness = pull_request_with_links().await;
    harness.press(KeyCode::Char('l'));
    harness.wait_for(|app| app.link_index == Some(0)).await;

    harness.press(KeyCode::Tab);

    harness.wait_for(|app| app.link_index.is_none()).await;
}
