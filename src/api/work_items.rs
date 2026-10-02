use anyhow::{Context, Result};
use serde_json::json;

use super::models::{
    CommentList, DetailComment, Iteration, ListResponse, SprintWorkItems, WiqlResult, WorkItem,
    WorkItemDetails, WorkItemResponse, WorkItemTypeCategory,
};
use super::{AdoClient, DEV_AZURE};

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
        let types = if types.is_empty() {
            String::new()
        } else {
            let types: Vec<String> = types.iter().map(|t| quote(t)).collect();
            format!("AND [System.WorkItemType] IN ({}) ", types.join(", "))
        };
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
            self.get(DEV_AZURE, &item_path, &[("$expand", "relations")]),
            self.get_versioned(
                DEV_AZURE,
                &comments_path,
                &comments_query,
                COMMENTS_API_VERSION,
            ),
        )?;
        let child_ids: Vec<u32> = item
            .relations
            .iter()
            .filter(|relation| relation.rel == CHILD_LINK)
            .filter_map(|relation| relation.url.rsplit('/').next()?.parse().ok())
            .collect();
        let children = self
            .work_items_batch(organization, project, &child_ids)
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
                DEV_AZURE,
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
        let (hidden, visible): (Vec<_>, Vec<_>) = categories
            .value
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
        Ok(types)
    }

    async fn current_iteration(
        &self,
        organization: &str,
        project: &str,
        team: &str,
    ) -> Result<Iteration> {
        let iterations: ListResponse<Iteration> = self
            .get(
                DEV_AZURE,
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
                DEV_AZURE,
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
                DEV_AZURE,
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

fn html_to_text(html: &str) -> String {
    let text =
        html2text::from_read(html.as_bytes(), HTML_TEXT_WIDTH).unwrap_or_else(|_| html.into());
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
