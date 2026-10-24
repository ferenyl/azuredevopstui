use crossterm::event::KeyCode;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{connected, expect_patch, select, wait_for_popup};
use crate::app::{Panel, Popup};

const WORK_ITEM_301: &str = "/contoso/Fabrikam/_apis/wit/workitems/301";
const BATCH: &str = "/contoso/Fabrikam/_apis/wit/workitemsbatch";

/// Work item 301 as the detail GET returns it, with `changes` merged into its fields.
fn item_301(changes: Value) -> Value {
    let mut fields = json!({
        "System.Title": "Order list paging",
        "System.WorkItemType": "User Story",
        "System.State": "Active",
        "System.ChangedDate": "2026-10-21T10:00:00Z",
        "System.CreatedDate": "2026-10-01T10:00:00Z",
        "System.Tags": "backend"
    });
    merge(&mut fields, changes);
    json!({ "id": 301, "fields": fields })
}

fn merge(fields: &mut Value, changes: Value) {
    for (name, value) in changes.as_object().expect("fields") {
        fields[name] = value.clone();
    }
}

/// The batch of my work items with `changes` merged into the fields of 301.
fn mine_with(changes: Value) -> Value {
    let mut fields = json!({
        "Microsoft.VSTS.Common.Priority": 2,
        "System.AssignedTo": { "id": "me-id", "displayName": "Mona Me" }
    });
    merge(&mut fields, changes);
    json!({ "value": [item_301(fields), {
        "id": 302,
        "fields": {
            "System.Title": "Crash on empty cart",
            "System.WorkItemType": "Bug",
            "System.State": "New",
            "System.ChangedDate": "2026-10-22T10:00:00Z",
            "System.CreatedDate": "2026-10-01T10:00:00Z",
            "System.AssignedTo": { "id": "me-id", "displayName": "Mona Me" }
        }
    }]})
}

/// Serves 301 with `changes` once the open detail reloads.
async fn reload_detail_with(harness: &Harness, changes: Value) {
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(WORK_ITEM_301)),
            ResponseTemplate::new(200).set_body_json(item_301(changes)),
            1..,
        )
        .await;
}

/// Serves my work items with `changes` to 301 once the sprint reloads.
async fn reload_mine_with(harness: &Harness, changes: Value) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(BATCH))
                .and(body_string_contains("\"ids\":[301,302]")),
            ResponseTemplate::new(200).set_body_json(mine_with(changes)),
            1..,
        )
        .await;
}

/// Connects, opens 301 from My work items and picks menu entry `row`.
async fn choose(row: usize) -> Harness {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Order list paging").await;
    harness.settle().await;
    harness.press(KeyCode::Char('e'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::WorkItemActions { .. })).await;
    for _ in 0..row {
        harness.press(KeyCode::Char('j'));
    }
    harness.press(KeyCode::Enter);
    harness
}

/// The detail overview shows `value` on the row of `label`.
fn assert_field(harness: &Harness, label: &str, value: &str) {
    let screen = harness.screen();
    let row = screen
        .lines()
        .find_map(|line| line.split_once(&format!("│ {label} "))?.1.split('│').next())
        .unwrap_or_else(|| panic!("no {label} row:\n{screen}"));
    assert_eq!(row.trim().trim_start_matches("● "), value, "{screen}");
}

fn assert_no_error(harness: &Harness) {
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn changing_the_state_patches_it_and_shows_it_after_the_reload() {
    let mut harness = choose(0).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::WorkItemField { .. })).await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/System.State","value":"Resolved""#,
    )
    .await;
    reload_detail_with(&harness, json!({ "System.State": "Resolved" })).await;

    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert_field(&harness, "State", "Resolved");
    assert_no_error(&harness);
}

#[tokio::test]
async fn editing_the_title_patches_it_and_the_list_shows_it() {
    let mut harness = choose(1).await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/System.Title","value":"Paged order list""#,
    )
    .await;
    reload_mine_with(&harness, json!({ "System.Title": "Paged order list" })).await;

    for _ in 0.."Order list paging".len() {
        harness.press(KeyCode::Backspace);
    }
    harness.type_text("Paged  order list ");
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Paged order list").await;

    assert_no_error(&harness);
}

#[tokio::test]
async fn assigning_searches_unknown_people_and_patches_the_unique_name() {
    let mut harness = choose(2).await;
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/_apis/identities"))
                .and(query_param("filterValue", "zoe")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [{
                "id": "zoe-id",
                "providerDisplayName": "Zoe Lind",
                "properties": { "Account": { "$value": "zoe@contoso.com" } }
            }]})),
            1,
        )
        .await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/System.AssignedTo","value":"zoe@contoso.com""#,
    )
    .await;
    reload_detail_with(
        &harness,
        json!({ "System.AssignedTo": { "id": "zoe-id", "displayName": "Zoe Lind" } }),
    )
    .await;

    harness.type_text("zoe");
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Zoe Lind <zoe@contoso.com>").await;
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert_field(&harness, "Assigned to", "Zoe Lind");
    assert_no_error(&harness);
}

#[tokio::test]
async fn setting_the_priority_patches_a_number_and_shows_it() {
    let mut harness = choose(3).await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/Microsoft.VSTS.Common.Priority","value":3"#,
    )
    .await;
    reload_mine_with(&harness, json!({ "Microsoft.VSTS.Common.Priority": 3 })).await;

    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert_field(&harness, "Priority", "3");
    assert_no_error(&harness);
}

#[tokio::test]
async fn moving_to_another_sprint_patches_the_iteration_path() {
    let mut harness = choose(4).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::WorkItemField { .. })).await;
    expect_patch(
        &harness,
        WORK_ITEM_301,
        r#""path":"/fields/System.IterationPath","value":"Fabrikam\\Sprint 40""#,
    )
    .await;
    reload_detail_with(
        &harness,
        json!({ "System.IterationPath": "Fabrikam\\Sprint 40" }),
    )
    .await;

    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    harness.settle().await;

    assert_field(&harness, "Sprint", "Fabrikam\\Sprint 40");
    assert_no_error(&harness);
}

#[tokio::test]
async fn n_creates_an_item_assigned_to_me_in_the_sprint_and_team_area() {
    let mut harness = connected().await;
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path("/contoso/Fabrikam/_apis/wit/workitems/$User%20Story"))
                .and(body_json(json!([
                    { "op": "add", "path": "/fields/System.Title", "value": "Export invoices" },
                    { "op": "add", "path": "/fields/System.AssignedTo", "value": "Mona Me" },
                    { "op": "add", "path": "/fields/System.IterationPath", "value": "Fabrikam\\Sprint 41" },
                    { "op": "add", "path": "/fields/System.AreaPath", "value": "Fabrikam\\Web" }
                ]))),
            ResponseTemplate::new(200).set_body_json(json!({ "id": 900 })),
            1,
        )
        .await;
    let sprint_loads = harness.ado.received("POST", "/wit/wiql").await;

    harness.press(KeyCode::Char('n'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::NewWorkItem { .. })).await;
    harness.wait_for_text("Test Bug").await;
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Title of the new User Story").await;
    harness.type_text("Export invoices");
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| app.notice() == Some("created #900"))
        .await;
    harness.settle().await;

    assert!(harness.ado.received("POST", "/wit/wiql").await > sprint_loads);
    assert_no_error(&harness);
}

#[tokio::test]
async fn a_child_task_is_linked_to_its_parent() {
    let mut harness = choose(5).await;
    wait_for_popup(&mut harness, |p| matches!(p, Popup::NewWorkItem { .. })).await;
    harness.wait_for_text("Test Bug").await;
    harness.press(KeyCode::Enter);
    harness
        .wait_for_text("Title of the new Task under #301")
        .await;

    harness.type_text("Write paging tests");
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| app.notice() == Some("created #902"))
        .await;
    harness.settle().await;

    assert_eq!(harness.ado.received("POST", "/workitems/$Task").await, 1);
    assert_no_error(&harness);
}

#[tokio::test]
async fn esc_at_any_step_sends_nothing() {
    let mut harness = connected().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    for row in 0..6 {
        harness.press(KeyCode::Char('e'));
        wait_for_popup(&mut harness, |p| matches!(p, Popup::WorkItemActions { .. })).await;
        for _ in 0..row {
            harness.press(KeyCode::Char('j'));
        }
        harness.press(KeyCode::Enter);
        harness.wait_for(|app| app.popup.is_some()).await;
        harness.press(KeyCode::Esc);
        if matches!(harness.app.popup, Some(Popup::Confirm { .. })) {
            harness.press(KeyCode::Char('y'));
        }
        assert!(harness.app.popup.is_none(), "menu row {row}");
    }
    harness.press(KeyCode::Char('e'));
    harness.press(KeyCode::Esc);
    harness.press(KeyCode::Char('n'));
    wait_for_popup(&mut harness, |p| matches!(p, Popup::NewWorkItem { .. })).await;
    harness.press(KeyCode::Esc);
    harness.settle().await;

    let requests = harness.ado.requests().await;
    assert!(
        requests
            .iter()
            .all(|r| !r.starts_with("PATCH") && !r.contains("/workitems/$")),
        "{requests:?}"
    );
    assert_no_error(&harness);
}
