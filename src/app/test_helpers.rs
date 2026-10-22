use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::api::{Board, CurrentUser};
use crate::app::{App, Data, Panel, Screen};
use crate::test_support::{config, pull_request, work_item};

pub(super) fn app() -> App {
    let mut app = App::new(Some(config()), None);
    app.screen = Screen::Main;
    app.focus = Panel::MyPrs;
    app.data = Data {
        my_prs: Some(vec![
            pull_request(1, "First", "me", "2026-10-03T10:00:00Z"),
            pull_request(2, "Second", "me", "2026-10-02T10:00:00Z"),
            pull_request(3, "Third", "me", "2026-10-01T10:00:00Z"),
        ]),
        other_prs: Some(vec![pull_request(4, "Theirs", "u", "2026-10-01T10:00:00Z")]),
        my_work_items: Some(vec![
            work_item(10, "Story", "User Story", "Active"),
            work_item(11, "Bug", "Bug", "New"),
        ]),
        ready_work_items: Some(vec![work_item(12, "Ready", "User Story", "Ready")]),
        sprint_name: Some("Sprint 1".into()),
        ..Default::default()
    };
    app
}

pub(super) fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

pub(super) fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

pub(super) fn open_pull_request(app: &mut App) {
    press(app, KeyCode::Enter);
    assert!(app.detail.as_ref().is_some_and(|d| d.is_pull_request(1)));
}

pub(super) fn me() -> CurrentUser {
    serde_json::from_value(serde_json::json!({ "id": "me-id", "providerDisplayName": "Me" }))
        .unwrap()
}

pub(super) fn board() -> Board {
    Board {
        column_field: "Column".into(),
        done_field: "Column.Done".into(),
        columns: serde_json::from_value(serde_json::json!([
            { "name": "Ready", "stateMappings": { "User Story": "New", "Bug": "New" } },
            { "name": "Active", "isSplit": true, "stateMappings": { "User Story": "Active", "Bug": "Active" } }
        ]))
        .unwrap(),
    }
}

pub(super) fn assign_first_item_to(app: &mut App, id: &str) {
    let item = &mut app.data.my_work_items.as_mut().unwrap()[0];
    item.fields.assigned_to = Some(
        serde_json::from_value(serde_json::json!({ "id": id, "displayName": "Someone" })).unwrap(),
    );
}
