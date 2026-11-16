use crossterm::event::KeyCode;
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::Harness;
use super::flows::{
    COMMENTS_301, PR_101, connected, requests_after_confirming, select, wait_for_popup,
};
use crate::app::{DetailInfo, DetailTab, Panel, Popup};

const WEB_MEMBERS: &str = "/teams/web-id/members";
const MOBILE_MEMBERS: &str = "/teams/mobile-id/members";

/// Opens the comment popup of work item 301.
async fn comment_on_301(harness: &mut Harness) {
    select(harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Char('m'));
    wait_for_popup(harness, |p| matches!(p, Popup::Text { .. })).await;
}

/// Opens PR 101 and starts a new thread on it.
async fn new_thread_on_101(harness: &mut Harness) {
    select(harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.press(KeyCode::Tab);
    harness.press(KeyCode::Char('m'));
    wait_for_popup(harness, |p| matches!(p, Popup::Text { .. })).await;
}

async fn wait_for_suggestions(harness: &mut Harness) {
    harness
        .wait_for(|app| {
            app.mentions
                .picker
                .as_ref()
                .is_some_and(|picker| !picker.matches.is_empty())
        })
        .await;
}

fn suggested(harness: &Harness) -> Vec<String> {
    harness
        .app
        .mentions
        .picker
        .as_ref()
        .map(|picker| {
            picker
                .matches
                .iter()
                .map(|member| member.display_name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn typed(harness: &Harness) -> String {
    match &harness.app.popup {
        Some(Popup::Text { text, .. }) => text.as_str().to_string(),
        _ => panic!("text popup not open"),
    }
}

async fn expect_post(harness: &Harness, target: &str, body_part: &str) {
    harness
        .ado
        .expect(
            Mock::given(method("POST"))
                .and(path(target))
                .and(body_string_contains(body_part)),
            ResponseTemplate::new(200).set_body_json(json!({ "id": 9 })),
            1,
        )
        .await;
}

fn member_names(harness: &Harness) -> Vec<String> {
    let members = harness
        .app
        .mentions
        .members
        .clone()
        .expect("members loaded");
    members.into_iter().map(|m| m.display_name).collect()
}

#[tokio::test]
async fn members_of_every_team_and_its_groups_are_loaded_once_at_startup() {
    let mut harness = connected().await;
    harness.settle().await;

    // Olle is only on a team through a group; groups and inactive people are left out.
    assert_eq!(
        member_names(&harness),
        ["Ann Berg", "Annika Lund", "Mona Me", "Olle Other"]
    );
    let identities = harness.ado.received("GET", "/_apis/identities").await;
    assert_eq!(identities, 2, "expand the teams, then resolve their people");
    assert_eq!(harness.ado.received("GET", WEB_MEMBERS).await, 0);

    comment_on_301(&mut harness).await;
    harness.type_text("@ol");
    wait_for_suggestions(&mut harness).await;
    harness.settle().await;

    assert_eq!(suggested(&harness), ["Olle Other"]);
    assert_eq!(
        harness.ado.received("GET", "/_apis/identities").await,
        identities
    );
}

#[tokio::test]
async fn without_identity_access_the_people_directly_on_the_teams_are_used() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .fail("GET", "/contoso/_apis/identities", 403)
        .await;
    harness.connect().await;
    harness.settle().await;

    assert_eq!(
        member_names(&harness),
        ["Ann Berg", "Annika Lund", "Mona Me"]
    );
    assert_eq!(harness.ado.received("GET", WEB_MEMBERS).await, 1);
    assert_eq!(harness.ado.received("GET", MOBILE_MEMBERS).await, 1);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

fn identity(id: &str, name: &str, account: &str) -> serde_json::Value {
    json!({
        "id": id,
        "providerDisplayName": name,
        "properties": { "Account": { "$value": account } }
    })
}

#[tokio::test]
async fn mention_groups_replace_the_team_members() {
    let mut harness = Harness::new().await;
    let mut config = (*harness.config()).clone();
    config.mention_groups = vec!["Developers".into(), "Nobody".into()];
    harness.app.config = Some(std::sync::Arc::new(config));
    harness.app.sync_api();
    let identities = || Mock::given(method("GET")).and(path("/contoso/_apis/identities"));
    let answer = |body| ResponseTemplate::new(200).set_body_json(body);
    // A person matching the name is not taken for the group.
    harness
        .ado
        .expect(
            identities().and(query_param("filterValue", "Developers")),
            answer(json!({ "value": [
                identity("fan-id", "Developers Fan", "fan@contoso.com"),
                {
                    "id": "devs-id",
                    "providerDisplayName": "[contoso]\\Developers",
                    "isContainer": true
                }
            ]})),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            identities().and(query_param("filterValue", "Nobody")),
            answer(json!({ "value": [] })),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            identities()
                .and(query_param("identityIds", "devs-id"))
                .and(query_param("queryMembership", "ExpandedDown")),
            answer(json!({ "value": [{
                "id": "devs-id",
                "isContainer": true,
                "memberIds": ["olle-id", "annika-id"]
            }]})),
            1,
        )
        .await;
    harness
        .ado
        .expect(
            identities()
                .and(query_param("identityIds", "annika-id,olle-id"))
                .and(query_param("queryMembership", "None")),
            answer(json!({ "value": [
                identity("annika-id", "Annika Lund", "annika@contoso.com"),
                identity("olle-id", "Olle Other", "olle@contoso.com")
            ]})),
            1,
        )
        .await;

    harness.connect().await;
    harness.settle().await;

    assert_eq!(member_names(&harness), ["Annika Lund", "Olle Other"]);
    assert_eq!(harness.ado.received("GET", "/Fabrikam/teams").await, 0);
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn unknown_mention_groups_show_an_error_when_typing_at() {
    let mut harness = Harness::new().await;
    let mut config = (*harness.config()).clone();
    config.mention_groups = vec!["Nobody".into()];
    harness.app.config = Some(std::sync::Arc::new(config));
    harness.app.sync_api();
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/_apis/identities"))
                .and(query_param("filterValue", "Nobody")),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [] })),
            2,
        )
        .await;
    harness.connect().await;
    harness.settle().await;
    assert!(harness.app.error.is_none(), "quiet at startup");

    comment_on_301(&mut harness).await;
    harness.type_text("@a");
    harness.wait_for(|app| app.error.is_some()).await;

    let error = harness.app.error.clone().unwrap();
    assert!(error.contains("group not found: Nobody"), "{error}");
}

#[tokio::test]
async fn typing_at_suggests_active_people_but_not_groups_or_removed_ones() {
    let mut harness = connected().await;
    comment_on_301(&mut harness).await;

    harness.type_text("Hi @an");
    wait_for_suggestions(&mut harness).await;

    assert_eq!(suggested(&harness), ["Ann Berg", "Annika Lund"]);
    let screen = harness.screen();
    assert!(screen.contains("@ mention"), "{screen}");
    assert!(screen.contains("annika@contoso.com"), "{screen}");
    assert!(!screen.contains("Anna Gone"), "{screen}");
    assert!(!screen.contains("Web Team"), "{screen}");

    harness.type_text("nika");
    assert_eq!(suggested(&harness), ["Annika Lund"]);
}

#[tokio::test]
async fn picking_someone_sends_their_id_in_a_work_item_comment() {
    let mut harness = connected().await;
    expect_post(&harness, COMMENTS_301, "Hi @<annika-id> please look").await;
    comment_on_301(&mut harness).await;
    harness.type_text("Hi @an");
    wait_for_suggestions(&mut harness).await;

    harness.press(KeyCode::Down);
    harness.press(KeyCode::Enter);

    assert_eq!(typed(&harness), "Hi @Annika Lund ");
    assert!(harness.app.mentions.picker.is_none());
    harness.type_text("please look");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("POST {COMMENTS_301}")),
        "{requests:?}"
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn picking_someone_sends_their_id_in_a_pull_request_thread() {
    let mut harness = connected().await;
    expect_post(&harness, &format!("{PR_101}/threads"), "@<ann-id>: ready?").await;
    new_thread_on_101(&mut harness).await;
    harness.type_text("@berg");
    wait_for_suggestions(&mut harness).await;
    assert_eq!(suggested(&harness), ["Ann Berg"]);

    harness.press(KeyCode::Enter);
    harness.press(KeyCode::Backspace);
    harness.type_text(": ready?");

    let requests = requests_after_confirming(&mut harness).await;

    assert!(
        requests.contains(&format!("POST {PR_101}/threads")),
        "{requests:?}"
    );
    assert!(harness.app.error.is_none(), "{:?}", harness.app.error);
}

#[tokio::test]
async fn esc_closes_the_suggestions_but_keeps_the_comment() {
    let mut harness = connected().await;
    comment_on_301(&mut harness).await;
    harness.type_text("@ann");
    wait_for_suggestions(&mut harness).await;

    harness.press(KeyCode::Esc);

    assert!(harness.app.mentions.picker.is_none());
    assert!(matches!(harness.app.popup, Some(Popup::Text { .. })));
    assert_eq!(typed(&harness), "@ann");
    assert!(!harness.screen().contains("@ mention"));

    harness.type_text("a");
    assert!(
        harness.app.mentions.picker.is_none(),
        "stays closed for this @"
    );
}

#[tokio::test]
async fn an_email_address_is_not_taken_for_a_mention() {
    let mut harness = connected().await;
    comment_on_301(&mut harness).await;

    harness.type_text("mail ann@con");
    harness.settle().await;

    assert!(harness.app.mentions.picker.is_none());
}

#[tokio::test]
async fn mentions_in_work_item_comments_show_names() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(COMMENTS_301)),
            ResponseTemplate::new(200).set_body_json(json!({
                "totalCount": 1,
                "comments": [{
                    "createdBy": { "id": "ann-id", "displayName": "Ann Berg" },
                    "createdDate": "2026-10-01T10:00:00Z",
                    "format": "markdown",
                    "text": "Ping @<ANNIKA-ID> and @<00000000-0000-0000-0000-0000000000ee>"
                }]
            })),
            1..,
        )
        .await;
    // Someone on no team is looked up by id, once.
    harness
        .ado
        .expect(
            Mock::given(method("GET"))
                .and(path("/contoso/_apis/identities"))
                .and(query_param(
                    "identityIds",
                    "00000000-0000-0000-0000-0000000000ee",
                )),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [{
                "id": "00000000-0000-0000-0000-0000000000ee",
                "providerDisplayName": "Eve External"
            }]})),
            1,
        )
        .await;
    harness.connect().await;
    select(&mut harness, Panel::MyWorkItems, 301);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::WorkItem(_))))
        .await;
    harness.app.select_tab(DetailTab::Comments);

    harness
        .wait_for_text("Ping @Annika Lund and @Eve External")
        .await;

    assert!(!harness.screen().contains("@<"), "{}", harness.screen());

    // Opening it again uses the cached name.
    harness.press(KeyCode::Char('r'));
    harness.settle().await;
}

#[tokio::test]
async fn mentions_in_pull_request_threads_show_names() {
    let mut harness = Harness::new().await;
    harness
        .ado
        .expect(
            Mock::given(method("GET")).and(path(format!("{PR_101}/threads"))),
            ResponseTemplate::new(200).set_body_json(json!({ "value": [{
                "id": 2,
                "status": "active",
                "comments": [{
                    "id": 2,
                    "author": { "id": "other-id", "displayName": "Olle Other" },
                    "content": "@<ann-id> can you check this, @<me-id>?",
                    "publishedDate": "2026-10-20T11:00:00Z",
                    "commentType": "text"
                }]
            }]})),
            1..,
        )
        .await;
    harness.connect().await;
    harness.settle().await;
    let at_startup = harness.ado.received("GET", "/_apis/identities").await;
    select(&mut harness, Panel::MyPrs, 101);
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::PullRequest(_))))
        .await;
    harness.press(KeyCode::Tab);

    harness
        .wait_for_text("@Ann Berg can you check this, @Mona Me?")
        .await;

    // Both are project members, so nobody is looked up.
    assert_eq!(
        harness.ado.received("GET", "/_apis/identities").await,
        at_startup
    );
}
