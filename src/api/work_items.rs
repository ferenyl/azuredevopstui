use anyhow::{Context, Result};
use futures::{StreamExt, future, stream};
use serde_json::json;

use super::ProjectApi;
use super::models::{
    CommentList, DetailComment, Iteration, Link, LinkTarget, ListResponse, Mention, Partial,
    PullRequest, Relation, SprintWorkItems, WiqlResult, WorkItem, WorkItemComment, WorkItemDetails,
    WorkItemResponse, WorkItemTypeCategory,
};
use crate::rich_text::RichText;

const MAX_WORK_ITEMS: &str = "200";
/// Most ids `workitemsbatch` accepts per request.
const BATCH_SIZE: usize = 200;
const FIELDS: [&str; 10] = [
    "System.Title",
    "System.WorkItemType",
    "System.State",
    "System.BoardColumn",
    "System.BoardColumnDone",
    "Microsoft.VSTS.Common.Priority",
    "System.ChangedDate",
    "System.CreatedDate",
    "System.AssignedTo",
    "System.Tags",
];
const CHILD_LINK: &str = "System.LinkTypes.Hierarchy-Forward";
const ARTIFACT_LINK: &str = "ArtifactLink";
const HYPERLINK: &str = "Hyperlink";
const ATTACHED_FILE: &str = "AttachedFile";
const PULL_REQUEST_ARTIFACT: &str = "vstfs:///Git/PullRequestId/";
const HIDDEN_CATEGORY: &str = "Microsoft.HiddenCategory";
const COMMENTS_API_VERSION: &str = "7.1-preview.4";
const MAX_COMMENTS: &str = "10";

impl ProjectApi {
    pub async fn sprint_work_items(
        &self,
        sprint: Option<&str>,
        ready_column: &str,
        types: &[String],
    ) -> Result<SprintWorkItems> {
        let iterations = self.iterations().await?;
        let iteration = match sprint {
            Some(sprint) => iterations
                .into_iter()
                .find(|iteration| iteration.path == sprint)
                .with_context(|| format!("sprint not found: {sprint}"))?,
            None => iterations
                .into_iter()
                .find(Iteration::is_current)
                .context("team has no current sprint")?,
        };
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
        let (mine, ready) =
            tokio::try_join!(self.query_work_items(&mine), self.query_work_items(&ready),)?;
        Ok(SprintWorkItems {
            iteration_name: iteration.name,
            start_date: iteration.attributes.start_date,
            finish_date: iteration.attributes.finish_date,
            mine,
            ready,
        })
    }

    pub async fn work_item_details(&self, id: u32) -> Result<WorkItemDetails> {
        let item_path = [
            &self.organization,
            &self.project,
            "_apis",
            "wit",
            "workitems",
            &id.to_string(),
        ];
        let (item, comments): (WorkItemResponse, _) = tokio::try_join!(
            self.client.get(
                &self.client.urls.dev_azure,
                &item_path,
                &[("$expand", "relations")]
            ),
            self.comments(id),
        )?;
        let linked_ids: Vec<u32> = item.relations.iter().filter_map(work_item_id).collect();
        let (linked, pull_requests) = tokio::join!(
            self.work_items_batch(&linked_ids),
            self.linked_pull_requests(&item.relations),
        );
        let linked = linked?;
        let child_ids = child_ids(&item.relations);
        let children = linked
            .iter()
            .filter(|item| child_ids.contains(&item.id))
            .cloned()
            .collect();
        let links = links(&item.relations, &linked);
        let formats = item.multiline_fields_format;
        let rich_text = |name: &str, value: Option<String>| {
            value.map(|value| to_rich_text(&value, formats.get(name).map(String::as_str)))
        };
        let fields = item.fields;
        Ok(WorkItemDetails {
            children,
            pull_requests,
            links,
            state: fields.state,
            board_column: fields.board_column,
            board_column_done: fields.board_column_done.unwrap_or(false),
            assigned_to: fields.assigned_to.map(|identity| identity.display_name),
            iteration_path: fields.iteration_path,
            tags: fields.tags,
            description: rich_text("System.Description", fields.description),
            acceptance_criteria: rich_text(
                "Microsoft.VSTS.Common.AcceptanceCriteria",
                fields.acceptance_criteria,
            ),
            repro_steps: rich_text("Microsoft.VSTS.TCM.ReproSteps", fields.repro_steps),
            comment_count: comments.total_count,
            comments: comments
                .comments
                .into_iter()
                .map(|comment| DetailComment {
                    author: comment.created_by.display_name,
                    date: comment.created_date,
                    text: to_rich_text(&comment.text, comment.format.as_deref()),
                })
                .collect(),
        })
    }

    /// Linked pull requests that could be loaded; others are skipped.
    async fn linked_pull_requests(&self, relations: &[Relation]) -> Vec<PullRequest> {
        let links = pull_request_links(relations);
        let results = future::join_all(
            links
                .iter()
                .map(|(project, repository, id)| self.pull_request(project, repository, *id)),
        )
        .await;
        results
            .into_iter()
            .filter_map(|result| {
                result
                    .inspect_err(|err| tracing::warn!("failed to load linked PR: {err:#}"))
                    .ok()
            })
            .collect()
    }

    /// Open work items recently mentioning the user in a comment they have not answered.
    pub async fn mentioned_work_items(&self, user_id: &str) -> Result<Partial<Mention>> {
        let wiql = "SELECT [System.Id] FROM WorkItems \
                    WHERE [System.Id] IN (@RecentMentions) \
                    AND [System.State] NOT IN ('Closed', 'Removed') \
                    ORDER BY [System.ChangedDate] DESC";
        let items = self.query_work_items(wiql).await?;
        let results: Vec<Result<Option<Mention>>> = stream::iter(items)
            .map(|item| async move {
                let comments = self.comments(item.id).await.inspect_err(|err| {
                    tracing::warn!(id = item.id, "skipped work item without comments: {err:#}");
                })?;
                let date = unanswered_mention(&comments.comments, user_id);
                Ok(date.map(|date| Mention { item, date }))
            })
            .buffered(8)
            .collect()
            .await;
        let failed = results.iter().filter(|result| result.is_err()).count();
        Ok(Partial {
            items: results.into_iter().flatten().flatten().collect(),
            failed,
        })
    }

    pub async fn add_work_item_comment(&self, id: u32, text: &str) -> Result<()> {
        let _: serde_json::Value = self
            .client
            .post_versioned(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "wit",
                    "workItems",
                    &id.to_string(),
                    "comments",
                ],
                &[("format", "markdown")],
                &json!({ "text": text }),
                COMMENTS_API_VERSION,
            )
            .await?;
        Ok(())
    }

    /// The newest comments first.
    async fn comments(&self, id: u32) -> Result<CommentList> {
        self.client
            .get_versioned(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "wit",
                    "workItems",
                    &id.to_string(),
                    "comments",
                ],
                &[
                    ("$top", MAX_COMMENTS),
                    ("order", "desc"),
                    ("$expand", "reactions"),
                ],
                COMMENTS_API_VERSION,
            )
            .await
    }

    /// Work item types in the project, without hidden ones like test plans and code reviews.
    pub async fn work_item_types(&self) -> Result<Vec<String>> {
        let categories: ListResponse<WorkItemTypeCategory> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "wit",
                    "workitemtypecategories",
                ],
                &[],
            )
            .await?;
        Ok(visible_types(categories.value))
    }

    /// The team's sprints, furthest in the future first.
    pub async fn iterations(&self) -> Result<Vec<Iteration>> {
        let iterations: ListResponse<Iteration> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    &self.team,
                    "_apis",
                    "work",
                    "teamsettings",
                    "iterations",
                ],
                &[],
            )
            .await?;
        let mut iterations = iterations.value;
        iterations.sort_by(|a, b| b.attributes.start_date.cmp(&a.attributes.start_date));
        Ok(iterations)
    }

    async fn query_work_items(&self, wiql: &str) -> Result<Vec<WorkItem>> {
        let result: WiqlResult = self
            .client
            .post(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    &self.team,
                    "_apis",
                    "wit",
                    "wiql",
                ],
                &[("$top", MAX_WORK_ITEMS)],
                &json!({ "query": wiql }),
            )
            .await?;
        if result.work_items.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<u32> = result.work_items.iter().map(|item| item.id).collect();
        self.work_items_batch(&ids).await
    }

    /// Fetches the given work items, keeping the order of `ids`; deleted or unreadable ones are skipped.
    pub(super) async fn work_items_batch(&self, ids: &[u32]) -> Result<Vec<WorkItem>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut work_items = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH_SIZE) {
            let batch: ListResponse<Option<WorkItem>> = self
                .client
                .post(
                    &self.client.urls.dev_azure,
                    &[
                        &self.organization,
                        &self.project,
                        "_apis",
                        "wit",
                        "workitemsbatch",
                    ],
                    &[],
                    &json!({ "ids": chunk, "fields": FIELDS, "errorPolicy": "omit" }),
                )
                .await?;
            work_items.extend(batch.value.into_iter().flatten());
        }
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

/// Date of the newest comment by someone else mentioning the user that the user has not reacted to,
/// unless the user wrote later.
fn unanswered_mention(newest_first: &[WorkItemComment], user_id: &str) -> Option<String> {
    let user_id = user_id.to_lowercase();
    newest_first
        .iter()
        .take_while(|comment| !comment.created_by.id.eq_ignore_ascii_case(&user_id))
        .filter(|comment| !comment.reactions.iter().any(|r| r.is_current_user_engaged))
        .find(|comment| comment.text.to_lowercase().contains(&user_id))
        .map(|comment| comment.created_date.clone())
}

/// Project id, repository id and PR id of linked pull requests.
fn pull_request_links(relations: &[Relation]) -> Vec<(String, String, u32)> {
    relations
        .iter()
        .filter(|relation| relation.rel == ARTIFACT_LINK)
        .filter_map(|relation| {
            let link = relation.url.strip_prefix(PULL_REQUEST_ARTIFACT)?;
            let link = link.replace("%2F", "/").replace("%2f", "/");
            let mut parts = link.split('/');
            let (project, repository, id) = (parts.next()?, parts.next()?, parts.next()?);
            Some((
                project.to_string(),
                repository.to_string(),
                id.parse().ok()?,
            ))
        })
        .collect()
}

fn work_item_id(relation: &Relation) -> Option<u32> {
    relation
        .url
        .split_once("/_apis/wit/workItems/")?
        .1
        .parse()
        .ok()
}

/// Relations for the links tab; children and pull requests are shown elsewhere.
fn links(relations: &[Relation], work_items: &[WorkItem]) -> Vec<Link> {
    let mut links: Vec<Link> = relations
        .iter()
        .filter(|relation| {
            relation.rel != CHILD_LINK && !relation.url.starts_with(PULL_REQUEST_ARTIFACT)
        })
        .filter_map(|relation| {
            let name = relation.attributes.name.clone();
            let (category, target) = match relation.rel.as_str() {
                HYPERLINK => ("Hyperlinks".into(), LinkTarget::Text(relation.url.clone())),
                ATTACHED_FILE => ("Attachments".into(), LinkTarget::Text(name?)),
                _ => {
                    let target = match work_item_id(relation) {
                        Some(id) => LinkTarget::WorkItem(Box::new(
                            work_items.iter().find(|item| item.id == id)?.clone(),
                        )),
                        None => LinkTarget::Text(artifact_text(&relation.url)),
                    };
                    (name.unwrap_or_else(|| relation.rel.clone()), target)
                }
            };
            Some(Link { category, target })
        })
        .collect();
    links.sort_by(|a, b| a.category.cmp(&b.category));
    links
}

/// Readable target of an artifact link, such as a short commit hash or branch name.
fn artifact_text(url: &str) -> String {
    let url = url.replace("%2F", "/").replace("%2f", "/");
    let Some(path) = url.strip_prefix("vstfs:///") else {
        return url;
    };
    let mut parts = path.splitn(3, '/');
    let (Some(_), Some(kind), Some(id)) = (parts.next(), parts.next(), parts.next()) else {
        return url;
    };
    match kind {
        "Commit" => id
            .rsplit('/')
            .next()
            .unwrap_or(id)
            .chars()
            .take(8)
            .collect(),
        "Ref" => {
            let name = id.splitn(3, '/').nth(2).unwrap_or(id);
            name.strip_prefix("GB").unwrap_or(name).to_string()
        }
        _ => id.rsplit('/').next().unwrap_or(id).to_string(),
    }
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

fn to_rich_text(value: &str, format: Option<&str>) -> RichText {
    if format.is_some_and(|format| format.eq_ignore_ascii_case("markdown")) {
        RichText::from_markdown(value)
    } else {
        RichText::from_html(value)
    }
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
    use crate::api::models::RelationAttributes;
    use crate::test_support::{ids_of_items, work_item_json};

    fn relation(rel: &str, url: &str) -> Relation {
        Relation {
            rel: rel.into(),
            url: url.into(),
            attributes: RelationAttributes::default(),
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
        let text = to_rich_text(
            "<div>Hello <b>world</b> &amp; more</div><p>Second</p>",
            None,
        )
        .plain();

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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "name": "Sprint 1", "path": "MyProject\\Sprint 1", "attributes": { "timeFrame": "current" } }
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
        let client = ProjectApi::with_base_url(&server.uri()).with_team("My Team");

        let sprint = client
            .sprint_work_items(None, "Ready", &["Bug".into()])
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
        let client = ProjectApi::with_base_url(&server.uri());

        let err = client
            .sprint_work_items(None, "Ready", &[])
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
        let client = ProjectApi::with_base_url(&server.uri());

        let details = client.work_item_details(10).await.unwrap();

        assert_eq!(details.state, "Active");
        assert!(details.board_column_done);
        assert_eq!(details.assigned_to.as_deref(), Some("Anna"));
        assert_eq!(
            details.description.map(|text| text.plain()).as_deref(),
            Some("Do the thing")
        );
        assert_eq!(ids_of_items(&details.children), [11, 12]);
        assert_eq!(details.comment_count, 3);
        assert_eq!(details.comments[0].author, "Bo");
        assert_eq!(details.comments[0].text.plain(), "Looks good");
    }

    #[tokio::test]
    async fn work_items_batch_omits_errors_and_skips_missing_items() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .and(body_partial_json(json!({ "errorPolicy": "omit" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(2, "Two", "Task", "New"),
                null,
                work_item_json(1, "One", "Task", "New")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let items = client.work_items_batch(&[1, 9, 2]).await.unwrap();

        assert_eq!(ids_of_items(&items), [1, 2]);
    }

    #[tokio::test]
    async fn work_items_batch_chunks_ids_and_keeps_order() {
        let server = MockServer::start().await;
        let ids: Vec<u32> = (1..=201).collect();
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .and(body_partial_json(json!({ "ids": &ids[..200] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(200, "Last of first", "Task", "New"),
                work_item_json(1, "First", "Task", "New")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .and(body_partial_json(json!({ "ids": [201] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(201, "Second chunk", "Task", "New")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let items = client.work_items_batch(&ids).await.unwrap();

        assert_eq!(ids_of_items(&items), [1, 200, 201]);
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
        let client = ProjectApi::with_base_url(&server.uri());

        let details = client.work_item_details(10).await.unwrap();

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
        let client = ProjectApi::with_base_url(&server.uri());

        let types = client.work_item_types().await.unwrap();

        assert_eq!(types, ["Bug"]);
    }

    fn comment(author: &str, text: &str, date: &str, reacted: bool) -> WorkItemComment {
        serde_json::from_value(json!({
            "createdBy": { "id": author, "displayName": author },
            "createdDate": date,
            "text": text,
            "reactions": [{ "type": "like", "count": 1, "isCurrentUserEngaged": reacted }]
        }))
        .unwrap()
    }

    #[test]
    fn mention_without_my_reaction_is_unanswered() {
        let comments = [comment("u", "@<me> look", "2026-10-02", false)];

        let date = unanswered_mention(&comments, "me");

        assert_eq!(date.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn mention_with_my_reaction_is_answered() {
        let comments = [comment("u", "@<me> look", "2026-10-02", true)];

        let date = unanswered_mention(&comments, "me");

        assert!(date.is_none());
    }

    #[test]
    fn older_mention_without_reaction_stays_unanswered() {
        let comments = [
            comment("u", "@<me> again", "2026-10-03", true),
            comment("u", "@<me> look", "2026-10-02", false),
        ];

        let date = unanswered_mention(&comments, "me");

        assert_eq!(date.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn comment_without_reactions_field_is_parsed() {
        let comment: WorkItemComment = serde_json::from_value(json!({
            "createdBy": { "id": "u", "displayName": "u" },
            "createdDate": "2026-10-02",
            "text": "@<me> look"
        }))
        .unwrap();

        assert!(comment.reactions.is_empty());
    }

    #[tokio::test]
    async fn iterations_list_newest_first() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/contoso/MyProject/Team/_apis/work/teamsettings/iterations",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "name": "Old", "path": "P\\Old", "attributes": { "startDate": "2026-01-01T00:00:00Z" } },
                { "name": "New", "path": "P\\New", "attributes": { "startDate": "2026-03-01T00:00:00Z" } }
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let iterations = client.iterations().await.unwrap();

        let names: Vec<_> = iterations.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["New", "Old"]);
    }

    #[tokio::test]
    async fn mentioned_work_items_keep_unanswered_mentions() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/Team/_apis/wit/wiql"))
            .and(body_string_contains("@RecentMentions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "workItems": [{ "id": 10 }, { "id": 11 }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(10, "Mentioned", "Task", "Active"),
                work_item_json(11, "Answered", "Task", "Active")
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/10/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "totalCount": 1,
                "comments": [{
                    "createdBy": { "id": "other", "displayName": "Bo" },
                    "createdDate": "2026-10-01T10:00:00Z",
                    "text": "ping @<me-id>"
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/11/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "totalCount": 2,
                "comments": [
                    { "createdBy": { "id": "me-id", "displayName": "Me" },
                      "createdDate": "2026-10-02T10:00:00Z", "text": "on it" },
                    { "createdBy": { "id": "other", "displayName": "Bo" },
                      "createdDate": "2026-10-01T10:00:00Z", "text": "ping @<me-id>" }
                ]
            })))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let mentions = client.mentioned_work_items("me-id").await.unwrap();

        assert_eq!(mentions.failed, 0);
        assert_eq!(mentions.items.len(), 1);
        assert_eq!(mentions.items[0].item.id, 10);
        assert_eq!(mentions.items[0].date, "2026-10-01T10:00:00Z");
    }

    #[tokio::test]
    async fn mentioned_work_items_skip_items_whose_comments_fail() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/Team/_apis/wit/wiql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "workItems": [{ "id": 10 }, { "id": 11 }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workitemsbatch"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                work_item_json(10, "Mentioned", "Task", "Active"),
                work_item_json(11, "Forbidden", "Task", "Active")
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/10/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "totalCount": 1,
                "comments": [{
                    "createdBy": { "id": "other", "displayName": "Bo" },
                    "createdDate": "2026-10-01T10:00:00Z",
                    "text": "ping @<me-id>"
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/11/comments"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let mentions = client.mentioned_work_items("me-id").await.unwrap();

        assert_eq!(mentions.failed, 1);
        assert_eq!(mentions.items.len(), 1);
        assert_eq!(mentions.items[0].item.id, 10);
    }

    #[tokio::test]
    async fn add_work_item_comment_posts_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/wit/workItems/10/comments"))
            .and(query_param("format", "markdown"))
            .and(body_partial_json(json!({ "text": "hello" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 1 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        client.add_work_item_comment(10, "hello").await.unwrap();
    }
}
