use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::models::{Backlog, Board, BoardResponse, ListResponse};
use super::{AdoClient, DEV_AZURE};

const REQUIREMENT_BACKLOG: &str = "requirement";

impl AdoClient {
    /// Columns of the team's requirement-level board, in board order.
    pub async fn board_columns(
        &self,
        organization: &str,
        project: &str,
        team: &str,
    ) -> Result<Vec<String>> {
        let board = self.board(organization, project, team).await?;
        Ok(board.columns.into_iter().map(|c| c.name).collect())
    }

    pub async fn board(&self, organization: &str, project: &str, team: &str) -> Result<Board> {
        let backlogs: ListResponse<Backlog> = self
            .get(
                DEV_AZURE,
                &[organization, project, team, "_apis", "work", "backlogs"],
                &[],
            )
            .await?;
        let backlog = backlogs
            .value
            .into_iter()
            .find(|b| b.kind.eq_ignore_ascii_case(REQUIREMENT_BACKLOG))
            .context("team has no requirement backlog")?;
        let board: BoardResponse = self
            .get(
                DEV_AZURE,
                &[
                    organization,
                    project,
                    team,
                    "_apis",
                    "work",
                    "boards",
                    &backlog.name,
                ],
                &[],
            )
            .await?;
        Ok(Board {
            column_field: board.fields.column_field.reference_name,
            done_field: board.fields.done_field.reference_name,
            columns: board.columns,
        })
    }

    pub async fn move_work_item(
        &self,
        organization: &str,
        project: &str,
        id: u32,
        board: &Board,
        target: &ColumnTarget,
    ) -> Result<()> {
        self.update_work_item(
            organization,
            project,
            id,
            &[
                ("System.State", json!(target.state)),
                (&board.column_field, json!(target.column)),
                (&board.done_field, json!(target.done)),
            ],
        )
        .await
    }

    pub async fn assign_work_item(
        &self,
        organization: &str,
        project: &str,
        id: u32,
        assignee: &str,
    ) -> Result<()> {
        self.update_work_item(
            organization,
            project,
            id,
            &[("System.AssignedTo", json!(assignee))],
        )
        .await
    }

    async fn update_work_item(
        &self,
        organization: &str,
        project: &str,
        id: u32,
        fields: &[(&str, Value)],
    ) -> Result<()> {
        let operations: Vec<Value> = fields
            .iter()
            .map(|(field, value)| json!({ "op": "add", "path": format!("/fields/{field}"), "value": value }))
            .collect();
        let id = id.to_string();
        let _: serde::de::IgnoredAny = self
            .patch(
                DEV_AZURE,
                &[organization, project, "_apis", "wit", "workitems", &id],
                &Value::Array(operations),
            )
            .await?;
        Ok(())
    }
}

/// A board position: a column, and for split columns whether it is the done half.
#[derive(Debug, Clone)]
pub struct ColumnTarget {
    pub label: String,
    pub column: String,
    pub done: bool,
    pub state: String,
}

impl Board {
    /// Positions available for a work item type, `None` when the type is not on the board.
    pub fn targets(&self, work_item_type: &str) -> Option<Vec<ColumnTarget>> {
        let mut targets = Vec::new();
        for column in &self.columns {
            let state = column.state_mappings.get(work_item_type)?;
            let target = |label: String, done: bool| ColumnTarget {
                label,
                column: column.name.clone(),
                done,
                state: state.clone(),
            };
            if column.is_split {
                targets.push(target(format!("{} (Doing)", column.name), false));
                targets.push(target(format!("{} (Done)", column.name), true));
            } else {
                targets.push(target(column.name.clone(), false));
            }
        }
        Some(targets)
    }
}
