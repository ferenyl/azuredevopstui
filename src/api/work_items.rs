use anyhow::{Context, Result};
use serde_json::json;

use super::AdoClient;
use super::models::{
    CommentList, DetailComment, Iteration, ListResponse, Relation, SprintWorkItems, WiqlResult,
    WorkItem, WorkItemDetails, WorkItemResponse, WorkItemTypeCategory,
};

const MAX_WORK_ITEMS: &str = "200";
const FIELDS: [&str; 9] = [
    "System.Title",
    "System.WorkItemType",
    "System.State",
    "System.BoardColumn",
    "System.BoardColumnDone",
    "Microsoft.VSTS.Common.Priority",
    "System.ChangedDate",
    "System.CreatedDate",
    "System.AssignedTo",
];
const CHILD_LINK: &str = "System.LinkTypes.Hierarchy-Forward";
const HIDDEN_CATEGORY: &str = "Microsoft.HiddenCategory";
const COMMENTS_API_VERSION: &str = "7.1-preview.4";
const MAX_COMMENTS: &str = "10";
const HTML_TEXT_WIDTH: usize = 10_000;

impl AdoClient {
    pub async fn sprint_work_items(
        &self,
        organization: &str,
        project: &str,
        team: &str,
        ready_column: &str,
        types: &[String],
    ) -> Result<SprintWorkItems> {
        let iteration = self.current_iteration(organization, project, team).await?;
        let path = quote(&iteration.path);
        let types = type_filter(types);
        let mine = format!(
            "SELECT [System.Id] FROM WorkItems \
             WHERE [System.AssignedTo] = @Me \
             AND [System.State] NOT IN ('Closed', 'Removed') \
             AND [System.IterationPath] = {path} \
             {types}\
             ORDER BY [System.ChangedDate] DESC"
        );
        let ready = format!(
            "SELECT [System.Id] FROM WorkItems \
             WHERE [System.BoardColumn] = {} \
             AND [System.AssignedTo] <> @Me \
             AND [System.IterationPath] = {path} \
             {types}\
             ORDER BY [System.Id]",
            quote(ready_column)
        );
        let (mine, ready) = tokio::try_join!(
            self.query_work_items(organization, project, team, &mine),
            self.query_work_items(organization, project, team, &ready),
        )?;
        Ok(SprintWorkItems {
            iteration_name: iteration.name,
            mine,
            ready,
        })
    }

    pub async fn work_item_details(
        &self,
        organization: &str,
        project: &str,
        id: u32,
    ) -> Result<WorkItemDetails> {
        let id = id.to_string();
        let item_path = [organization, project, "_apis", "wit", "workitems", &id];
        let comments_path = [
            organization,
            project,
            "_apis",
            "wit",
            "workItems",
            &id,
            "comments",
        ];
        let comments_query = [("$top", MAX_COMMENTS), ("order", "desc")];
        let (item, comments): (WorkItemResponse, CommentList) = tokio::try_join!(
            self.get(
                &self.urls.dev_azure,
                &item_path,
                &[("$expand", "relations")]
            ),
            self.get_versioned(
                &self.urls.dev_azure,
                &comments_path,
                &comments_query,
                COMMENTS_API_VERSION,
            ),
        )?;
        let children = self
            .work_items_batch(organization, project, &child_ids(&item.relations))
            .await?;
        let fields = item.fields;
        Ok(WorkItemDetails {
            children,
            state: fields.state,
            board_column: fields.board_column,
            board_column_done: fields.board_column_done.unwrap_or(false),
            assigned_to: fields.assigned_to.map(|identity| identity.display_name),
            iteration_path: fields.iteration_path,
            tags: fields.tags,
            description: fields.description.as_deref().map(html_to_text),
            acceptance_criteria: fields.acceptance_criteria.as_deref().map(html_to_text),
            repro_steps: fields.repro_steps.as_deref().map(html_to_text),
            comment_count: comments.total_count,
            comments: comments
                .comments
                .into_iter()
                .map(|comment| DetailComment {
                    author: comment.created_by.display_name,
                    date: comment.created_date,
                    text: html_to_text(&comment.text),
                })
                .collect(),
        })
    }

    /// Work item types in the project, without hidden ones like test plans and code reviews.
    pub async fn work_item_types(&self, organization: &str, project: &str) -> Result<Vec<String>> {
        let categories: ListResponse<WorkItemTypeCategory> = self
            .get(
                &self.urls.dev_azure,
                &[
                    organization,
                    project,
                    "_apis",
                    "wit",
                    "workitemtypecategories",
                ],
                &[],
            )
            .await?;
        Ok(visible_types(categories.value))
    }

    async fn current_iteration(
        &self,
        organization: &str,
        project: &str,
        team: &str,
    ) -> Result<Iteration> {
        let iterations: ListResponse<Iteration> = self
            .get(
                &self.urls.dev_azure,
                &[
                    organization,
                    project,
                    team,
                    "_apis",
                    "work",
                    "teamsettings",
                    "iterations",
                ],
                &[("$timeframe", "current")],
            )
            .await?;
        iterations
            .value
            .into_iter()
            .next()
            .context("team has no current sprint")
    }

    async fn query_work_items(
        &self,
        organization: &str,
        project: &str,
        team: &str,
        wiql: &str,
    ) -> Result<Vec<WorkItem>> {
        let result: WiqlResult = self
            .post(
                &self.urls.dev_azure,
                &[organization, project, team, "_apis", "wit", "wiql"],
                &[("$top", MAX_WORK_ITEMS)],
                &json!({ "query": wiql }),
            )
            .await?;
        if result.work_items.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<u32> = result.work_items.iter().map(|item| item.id).collect();
        self.work_items_batch(organization, project, &ids).await
    }

    /// Fetches the given work items, keeping the order of `ids`.
    async fn work_items_batch(
        &self,
        organization: &str,
        project: &str,
        ids: &[u32],
    ) -> Result<Vec<WorkItem>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let batch: ListResponse<WorkItem> = self
            .post(
                &self.urls.dev_azure,
                &[organization, project, "_apis", "wit", "workitemsbatch"],
                &[],
                &json!({ "ids": ids, "fields": FIELDS }),
            )
            .await?;
        let mut work_items = batch.value;
        work_items.sort_by_key(|item| ids.iter().position(|id| *id == item.id));
        Ok(work_items)
    }
}

/// WIQL condition limiting the query to `types`; empty when all types are wanted.
fn type_filter(types: &[String]) -> String {
    if types.is_empty() {
        return String::new();
    }
    let types: Vec<String> = types.iter().map(|t| quote(t)).collect();
    format!("AND [System.WorkItemType] IN ({}) ", types.join(", "))
}

fn child_ids(relations: &[Relation]) -> Vec<u32> {
    relations
        .iter()
        .filter(|relation| relation.rel == CHILD_LINK)
        .filter_map(|relation| relation.url.rsplit('/').next()?.parse().ok())
        .collect()
}

/// Type names from all categories except hidden ones, sorted and deduplicated.
fn visible_types(categories: Vec<WorkItemTypeCategory>) -> Vec<String> {
    let (hidden, visible): (Vec<_>, Vec<_>) = categories
        .into_iter()
        .partition(|category| category.reference_name == HIDDEN_CATEGORY);
    let hidden: Vec<String> = hidden
        .into_iter()
        .flat_map(|category| category.work_item_types)
        .map(|kind| kind.name)
        .collect();
    let mut types: Vec<String> = visible
        .into_iter()
        .flat_map(|category| category.work_item_types)
        .map(|kind| kind.name)
        .filter(|name| !hidden.contains(name))
        .collect();
    types.sort();
    types.dedup();
    types
}

/// Plain text with `<img>` tags turned into image marker lines.
fn html_to_text(html: &str) -> String {
    let html = crate::images::extract_html_images(html);
    let text = html2text::from_read(html.as_bytes(), HTML_TEXT_WIDTH).unwrap_or(html);
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, body_string_contains, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::auth::Auth;
    use crate::test_support::{ids_of_items, work_item_json};

    fn relation(rel: &str, url: &str) -> Relation {
        Relation {
            rel: rel.into(),
            url: url.into(),
        }
    }

    fn category(reference_name: &str, types: &[&str]) -> WorkItemTypeCategory {
        serde_json::from_value(json!({
            "referenceName": reference_name,
            "workItemTypes": types.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    #[test]
    fn quote_doubles_single_quotes() {
        assert_eq!(quote("Don't"), "'Don''t'");
        assert_eq!(quote(r"MyProject\Sprint 1"), r"'MyProject\Sprint 1'");
    }

    #[test]
    fn html_to_text_strips_markup() {
        let text = html_to_text("<div>Hello <b>world</b> &amp; more</div><p>Second</p>");

        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(text.contains("& more"));
        assert!(text.contains("Second"));
        assert!(!text.contains('<'));
        assert!(text.lines().all(|line| line == line.trim_end()));
    }

    #[test]
    fn empty_type_filter_matches_all_types() {
        assert_eq!(type_filter(&[]), "");
    }

    #[test]
    fn type_filter_quotes_and_joins_types() {
        let filter = type_filter(&["User Story".into(), "Bug".into()]);

        assert_eq!(
            filter,
            "AND [System.WorkItemType] IN ('User Story', 'Bug') "
        );
    }

    #[test]
    fn child_ids_only_follow_forward_hierarchy_links() {
        let relations = [
            relation(
                "System.LinkTypes.Hierarchy-Reverse",
                "https://dev.azure.com/o/_apis/wit/workItems/1",
            ),
            relation(CHILD_LINK, "https://dev.azure.com/o/_apis/wit/workItems/12"),
            relation("ArtifactLink", "vstfs:///Build/Build/608369"),
            relation(CHILD_LINK, "https://dev.azure.com/o/_apis/wit/workItems/13"),
        ];

        assert_eq!(child_ids(&relations), [12, 13]);
    }

    #[test]
    fn visible_types_drop_hidden_types_everywhere() {
        let categories = vec![
            category("Microsoft.RequirementCategory", &["User Story", "Issue"]),
            category(
                "Microsoft.CodeReviewRequestCategory",
                &["Code Review Request"],
            ),
            category("Microsoft.TaskCategory", &["Task", "User Story"]),
            category(HIDDEN_CATEGORY, &["Code Review Request"]),
        ];

        assert_eq!(visible_types(categories), ["Issue", "Task", "User Story"]);
    }

    #[tokio::test]
    async fn sprint_work_items_keep_wiql_order_and_filter_types() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/contoso/MyProject/My%20Team/_apis/work/teamsettings/iterations",
            ))
            .and(query_param("$timeframe", "current"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "name": "Sprint 1", "path": "MyProject\\Sprint 1" }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/My%20Team/_apis/wit/wiql"))
            .and(body_string_contains("[System.AssignedTo] = @Me"))
            .and(body_string_contains("[System.WorkItemType] IN ('Bug')"))
            .and(body_string_contains(
                r"[System.IterationPath] = 'MyProject\\Sprint 1'",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "workItems": [{ "id": 3 }, { "id": 1 }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/My%20Team/_apis/wit/wiql"))
            .and(body_string_contains("[System.BoardColumn] = 'Ready'"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "workItems": [] })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .and(body_partial_json(json!({ "ids": [3, 1] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(1, "One", "Bug", "New"),
                work_item_json(3, "Three", "Bug", "Active")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let sprint = client
            .sprint_work_items("contoso", "MyProject", "My Team", "Ready", &["Bug".into()])
            .await
            .unwrap();

        assert_eq!(sprint.iteration_name, "Sprint 1");
        assert_eq!(ids_of_items(&sprint.mine), [3, 1]);
        assert!(sprint.ready.is_empty());
    }

    #[tokio::test]
    async fn sprint_work_items_fail_without_current_sprint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let err = client
            .sprint_work_items("contoso", "MyProject", "Team", "Ready", &[])
            .await
            .err()
            .expect("no current sprint");

        assert_eq!(err.to_string(), "team has no current sprint");
    }

    #[tokio::test]
    async fn work_item_details_load_children_and_comments() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/10"))
            .and(query_param("$expand", "relations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": {
                    "System.State": "Active",
                    "System.BoardColumn": "Active",
                    "System.BoardColumnDone": true,
                    "System.AssignedTo": { "id": "u", "displayName": "Anna" },
                    "System.Description": "<p>Do the <b>thing</b></p>"
                },
                "relations": [
                    { "rel": CHILD_LINK, "url": "https://x/_apis/wit/workItems/11" },
                    { "rel": CHILD_LINK, "url": "https://x/_apis/wit/workItems/12" }
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/10/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "totalCount": 3,
                "comments": [{
                    "createdBy": { "id": "u", "displayName": "Bo" },
                    "createdDate": "2026-10-01T10:00:00Z",
                    "text": "<div>Looks good</div>"
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .and(body_partial_json(json!({ "ids": [11, 12] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(12, "Second", "Task", "New"),
                work_item_json(11, "First", "Task", "Closed")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let details = client
            .work_item_details("contoso", "MyProject", 10)
            .await
            .unwrap();

        assert_eq!(details.state, "Active");
        assert!(details.board_column_done);
        assert_eq!(details.assigned_to.as_deref(), Some("Anna"));
        assert_eq!(details.description.as_deref(), Some("Do the **thing**"));
        assert_eq!(ids_of_items(&details.children), [11, 12]);
        assert_eq!(details.comment_count, 3);
        assert_eq!(details.comments[0].author, "Bo");
        assert_eq!(details.comments[0].text, "Looks good");
    }

    #[tokio::test]
    async fn work_item_without_children_skips_batch_call() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": { "System.State": "New" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/10/comments"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "totalCount": 0, "comments": [] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let details = client
            .work_item_details("contoso", "MyProject", 10)
            .await
            .unwrap();

        assert!(details.children.is_empty());
        assert!(details.comments.is_empty());
    }

    #[tokio::test]
    async fn work_item_types_come_from_visible_categories() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workitemtypecategories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "referenceName": "Microsoft.BugCategory", "workItemTypes": [{ "name": "Bug" }] },
                { "referenceName": HIDDEN_CATEGORY, "workItemTypes": [{ "name": "Test Plan" }] },
                { "referenceName": "Microsoft.TestPlanCategory", "workItemTypes": [{ "name": "Test Plan" }] }
            ]})))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let types = client
            .work_item_types("contoso", "MyProject")
            .await
            .unwrap();

        assert_eq!(types, ["Bug"]);
    }
}
