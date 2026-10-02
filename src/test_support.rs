use serde_json::{Value, json};

use crate::api::{PullRequest, WorkItem};
use crate::config::Config;

pub fn config() -> Config {
    let mut config = Config::new("contoso".into(), "MyProject".into(), "My Team".into());
    config.ready_column = Some("Ready".into());
    config
}

pub fn pull_request_json(id: u32, title: &str, creator_id: &str, created: &str) -> Value {
    json!({
        "pullRequestId": id,
        "title": title,
        "createdBy": { "id": creator_id, "displayName": format!("User {creator_id}") },
        "repository": { "id": "repo-id", "name": "my-api", "project": { "id": "project-id" } },
        "status": "active",
        "sourceRefName": "refs/heads/feature/x",
        "targetRefName": "refs/heads/main",
        "creationDate": created,
        "mergeStatus": "succeeded",
        "reviewers": [
            { "displayName": "Anna", "vote": 10, "isRequired": true },
            { "displayName": "Bo", "vote": 0 }
        ]
    })
}

pub fn pull_request(id: u32, title: &str, creator_id: &str, created: &str) -> PullRequest {
    serde_json::from_value(pull_request_json(id, title, creator_id, created))
        .expect("valid pull request")
}

pub fn pull_request_in(id: u32, repository: &str, created: &str) -> PullRequest {
    let mut pr = pull_request(id, &format!("PR {id}"), "other", created);
    pr.repository.name = repository.into();
    pr
}

pub fn work_item_json(id: u32, title: &str, kind: &str, state: &str) -> Value {
    json!({
        "id": id,
        "fields": {
            "System.Title": title,
            "System.WorkItemType": kind,
            "System.State": state,
            "System.BoardColumn": state,
            "System.ChangedDate": "2026-10-01T10:00:00Z",
            "System.CreatedDate": "2026-09-01T10:00:00Z"
        }
    })
}

pub fn work_item(id: u32, title: &str, kind: &str, state: &str) -> WorkItem {
    serde_json::from_value(work_item_json(id, title, kind, state)).expect("valid work item")
}

pub fn work_item_with(id: u32, priority: Option<u8>, changed: &str, created: &str) -> WorkItem {
    let mut item = work_item(id, &format!("Item {id}"), "User Story", "Active");
    item.fields.priority = priority;
    item.fields.changed_date = changed.into();
    item.fields.created_date = created.into();
    item
}

pub fn ids_of_prs(prs: &[PullRequest]) -> Vec<u32> {
    prs.iter().map(|pr| pr.pull_request_id).collect()
}

pub fn ids_of_items(items: &[WorkItem]) -> Vec<u32> {
    items.iter().map(|item| item.id).collect()
}
