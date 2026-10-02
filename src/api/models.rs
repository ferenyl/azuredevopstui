use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListResponse<T> {
    pub value: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionData {
    pub authenticated_user: CurrentUser,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: String,
    pub provider_display_name: String,
    #[serde(default)]
    pub properties: UserProperties,
}

impl CurrentUser {
    /// Email/UPN when available, used as identity value in work item fields.
    pub fn unique_name(&self) -> &str {
        self.properties
            .account
            .as_ref()
            .map_or(&self.provider_display_name, |account| &account.value)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserProperties {
    #[serde(rename = "Account")]
    pub account: Option<PropertyValue>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyValue {
    #[serde(rename = "$value")]
    pub value: String,
}

#[derive(Deserialize)]
pub struct Profile {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub account_name: String,
}

#[derive(Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Deserialize)]
pub struct BoardResponse {
    pub columns: Vec<BoardColumn>,
    pub fields: BoardFields,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFields {
    pub column_field: FieldReference,
    pub done_field: FieldReference,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldReference {
    pub reference_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardColumn {
    pub name: String,
    #[serde(default)]
    pub is_split: bool,
    #[serde(default)]
    pub state_mappings: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub column_field: String,
    pub done_field: String,
    pub columns: Vec<BoardColumn>,
}

#[derive(Deserialize)]
pub struct Backlog {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Deserialize)]
pub struct Identity {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRef {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub project: ProjectRef,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRef {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub pull_request_id: u32,
    pub title: String,
    pub created_by: IdentityRef,
    pub repository: Repository,
    #[serde(default)]
    pub is_draft: bool,
    pub status: String,
    pub source_ref_name: String,
    pub target_ref_name: String,
    pub creation_date: String,
    pub merge_status: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
    /// Reviewer policies met; only set for others' PRs.
    #[serde(skip)]
    pub approved: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reviewer {
    pub display_name: String,
    pub vote: i32,
    pub is_required: Option<bool>,
}

pub struct PullRequestDetails {
    pub threads: Vec<Thread>,
    pub statuses: Vec<PullRequestStatus>,
    pub policies: Vec<PolicyEvaluation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub status: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
    pub thread_context: Option<ThreadContext>,
    pub comments: Vec<Comment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadContext {
    pub file_path: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: IdentityRef,
    pub content: Option<String>,
    pub comment_type: Option<String>,
    pub published_date: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Deserialize)]
pub struct PullRequestStatus {
    pub id: u32,
    pub state: Option<String>,
    pub description: Option<String>,
    pub context: StatusContext,
}

#[derive(Deserialize)]
pub struct StatusContext {
    pub name: String,
    pub genre: Option<String>,
}

#[derive(Deserialize)]
pub struct PolicyEvaluation {
    pub status: String,
    pub configuration: PolicyConfiguration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyConfiguration {
    pub is_blocking: bool,
    #[serde(rename = "type")]
    pub kind: PolicyType,
    pub settings: PolicySettings,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyType {
    pub display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySettings {
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct Iteration {
    pub name: String,
    pub path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WiqlResult {
    pub work_items: Vec<WorkItemRef>,
}

#[derive(Deserialize)]
pub struct WorkItemRef {
    pub id: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItem {
    pub id: u32,
    pub fields: WorkItemFields,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemFields {
    #[serde(rename = "System.Title")]
    pub title: String,
    #[serde(rename = "System.WorkItemType")]
    pub work_item_type: String,
    #[serde(rename = "System.State")]
    pub state: String,
    #[serde(rename = "System.BoardColumn")]
    pub board_column: Option<String>,
    #[serde(rename = "System.BoardColumnDone")]
    pub board_column_done: Option<bool>,
    #[serde(rename = "Microsoft.VSTS.Common.Priority")]
    pub priority: Option<u8>,
    #[serde(rename = "System.ChangedDate", default)]
    pub changed_date: String,
    #[serde(rename = "System.CreatedDate", default)]
    pub created_date: String,
    #[serde(rename = "System.AssignedTo")]
    pub assigned_to: Option<IdentityRef>,
}

pub struct SprintWorkItems {
    pub iteration_name: String,
    pub mine: Vec<WorkItem>,
    pub ready: Vec<WorkItem>,
}

#[derive(Deserialize)]
pub struct WorkItemResponse {
    pub fields: WorkItemDetailFields,
    #[serde(default)]
    pub relations: Vec<Relation>,
}

#[derive(Deserialize)]
pub struct Relation {
    pub rel: String,
    pub url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemTypeCategory {
    pub reference_name: String,
    pub work_item_types: Vec<Named>,
}

#[derive(Deserialize)]
pub struct WorkItemDetailFields {
    #[serde(rename = "System.State")]
    pub state: String,
    #[serde(rename = "System.BoardColumn")]
    pub board_column: Option<String>,
    #[serde(rename = "System.BoardColumnDone")]
    pub board_column_done: Option<bool>,
    #[serde(rename = "System.AssignedTo")]
    pub assigned_to: Option<IdentityRef>,
    #[serde(rename = "System.IterationPath")]
    pub iteration_path: Option<String>,
    #[serde(rename = "System.Tags")]
    pub tags: Option<String>,
    #[serde(rename = "System.Description")]
    pub description: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.AcceptanceCriteria")]
    pub acceptance_criteria: Option<String>,
    #[serde(rename = "Microsoft.VSTS.TCM.ReproSteps")]
    pub repro_steps: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentList {
    pub total_count: u32,
    pub comments: Vec<WorkItemComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemComment {
    pub created_by: IdentityRef,
    pub created_date: String,
    pub text: String,
}

/// Work item fields not included in the list query, with HTML converted to text.
pub struct WorkItemDetails {
    pub state: String,
    pub board_column: Option<String>,
    pub board_column_done: bool,
    pub assigned_to: Option<String>,
    pub iteration_path: Option<String>,
    pub tags: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub repro_steps: Option<String>,
    pub comment_count: u32,
    pub comments: Vec<DetailComment>,
    pub children: Vec<WorkItem>,
}

pub struct DetailComment {
    pub author: String,
    pub date: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn unique_name_prefers_account_property() {
        let data: ConnectionData = serde_json::from_value(json!({
            "authenticatedUser": {
                "id": "user-id",
                "providerDisplayName": "Anna Andersson",
                "properties": { "Account": { "$type": "System.String", "$value": "anna@example.com" } }
            }
        }))
        .unwrap();

        assert_eq!(data.authenticated_user.id, "user-id");
        assert_eq!(data.authenticated_user.unique_name(), "anna@example.com");
    }

    #[test]
    fn unique_name_falls_back_to_display_name() {
        let user: CurrentUser = serde_json::from_value(json!({
            "id": "user-id",
            "providerDisplayName": "Anna Andersson"
        }))
        .unwrap();

        assert_eq!(user.unique_name(), "Anna Andersson");
    }

    #[test]
    fn work_item_allows_missing_optional_fields() {
        let item: WorkItem = serde_json::from_value(json!({
            "id": 1,
            "fields": {
                "System.Title": "Title",
                "System.WorkItemType": "Bug",
                "System.State": "New"
            }
        }))
        .unwrap();

        assert_eq!(item.fields.priority, None);
        assert_eq!(item.fields.board_column, None);
        assert!(item.fields.assigned_to.is_none());
        assert_eq!(item.fields.changed_date, "");
    }

    #[test]
    fn work_item_reads_all_fields() {
        let item: WorkItem = serde_json::from_value(json!({
            "id": 2,
            "fields": {
                "System.Title": "Title",
                "System.WorkItemType": "User Story",
                "System.State": "Active",
                "System.BoardColumn": "Active",
                "System.BoardColumnDone": true,
                "Microsoft.VSTS.Common.Priority": 2,
                "System.ChangedDate": "2026-10-01T10:00:00Z",
                "System.CreatedDate": "2026-09-01T10:00:00Z",
                "System.AssignedTo": { "id": "u", "displayName": "Anna" }
            }
        }))
        .unwrap();

        assert_eq!(item.fields.priority, Some(2));
        assert_eq!(item.fields.board_column_done, Some(true));
        assert_eq!(item.fields.assigned_to.unwrap().display_name, "Anna");
    }

    #[test]
    fn pull_request_reads_reviewers() {
        let pr = crate::test_support::pull_request(1, "Title", "creator", "2026-10-01T10:00:00Z");

        assert_eq!(pr.reviewers.len(), 2);
        assert_eq!(pr.reviewers[0].vote, 10);
        assert_eq!(pr.reviewers[0].is_required, Some(true));
        assert_eq!(pr.reviewers[1].is_required, None);
        assert!(!pr.is_draft);
    }

    #[test]
    fn board_column_defaults_to_not_split() {
        let column: BoardColumn = serde_json::from_value(json!({
            "name": "New",
            "stateMappings": { "User Story": "New", "Bug": "New" }
        }))
        .unwrap();

        assert!(!column.is_split);
        assert_eq!(column.state_mappings["Bug"], "New");
    }
}
