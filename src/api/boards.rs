use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::AdoClient;
use super::models::{Backlog, Board, BoardResponse, ListResponse};

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
                &self.urls.dev_azure,
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
                &self.urls.dev_azure,
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
        self.update_work_item(organization, project, id, &column_fields(board, target))
            .await
    }

    /// Clears the assignee and moves the item in one update.
    pub async fn unassign_work_item(
        &self,
        organization: &str,
        project: &str,
        id: u32,
        board: &Board,
        target: &ColumnTarget,
    ) -> Result<()> {
        let mut fields = vec![("System.AssignedTo", json!(""))];
        fields.extend(column_fields(board, target));
        self.update_work_item(organization, project, id, &fields)
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
                &self.urls.dev_azure,
                &[organization, project, "_apis", "wit", "workitems", &id],
                &Value::Array(operations),
            )
            .await?;
        Ok(())
    }
}

fn column_fields<'a>(board: &'a Board, target: &ColumnTarget) -> [(&'a str, Value); 3] {
    [
        ("System.State", json!(target.state)),
        (&board.column_field, json!(target.column)),
        (&board.done_field, json!(target.done)),
    ]
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::api::models::BoardColumn;
    use crate::auth::Auth;

    fn column(name: &str, is_split: bool, mappings: &[(&str, &str)]) -> BoardColumn {
        BoardColumn {
            name: name.into(),
            is_split,
            state_mappings: mappings
                .iter()
                .map(|(kind, state)| (kind.to_string(), state.to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    fn board(columns: Vec<BoardColumn>) -> Board {
        Board {
            column_field: "WEF_X_Kanban.Column".into(),
            done_field: "WEF_X_Kanban.Column.Done".into(),
            columns,
        }
    }

    #[test]
    fn plain_column_gives_one_target() {
        let board = board(vec![column("New", false, &[("User Story", "New")])]);

        let targets = board.targets("User Story").unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].label, "New");
        assert!(!targets[0].done);
        assert_eq!(targets[0].state, "New");
    }

    #[test]
    fn split_column_gives_doing_and_done_targets() {
        let board = board(vec![column("Active", true, &[("User Story", "Active")])]);

        let targets = board.targets("User Story").unwrap();

        let labels: Vec<_> = targets.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["Active (Doing)", "Active (Done)"]);
        assert_eq!(
            targets.iter().map(|t| t.done).collect::<Vec<_>>(),
            [false, true]
        );
        assert!(
            targets
                .iter()
                .all(|t| t.column == "Active" && t.state == "Active")
        );
    }

    #[test]
    fn type_missing_from_a_column_is_not_on_the_board() {
        let board = board(vec![
            column("New", false, &[("User Story", "New"), ("Bug", "New")]),
            column("Closed", false, &[("User Story", "Closed")]),
        ]);

        assert!(board.targets("Bug").is_none());
        assert_eq!(board.targets("User Story").unwrap().len(), 2);
    }

    #[tokio::test]
    async fn board_uses_the_requirement_backlog() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/Team/_apis/work/backlogs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "name": "Epics", "type": "portfolio" },
                { "name": "Stories", "type": "requirement" }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/Team/_apis/work/boards/Stories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "columns": [
                    { "name": "New", "stateMappings": { "User Story": "New" } },
                    { "name": "Active", "isSplit": true, "stateMappings": { "User Story": "Active" } }
                ],
                "fields": {
                    "columnField": { "referenceName": "WEF_A_Kanban.Column" },
                    "doneField": { "referenceName": "WEF_A_Kanban.Column.Done" }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let board = client.board("contoso", "MyProject", "Team").await.unwrap();

        assert_eq!(board.column_field, "WEF_A_Kanban.Column");
        assert_eq!(board.done_field, "WEF_A_Kanban.Column.Done");
        assert_eq!(board.columns.len(), 2);
        assert!(board.columns[1].is_split);
    }

    #[tokio::test]
    async fn move_work_item_patches_state_column_and_done() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/5"))
            .and(header("content-type", "application/json-patch+json"))
            .and(body_json(json!([
                { "op": "add", "path": "/fields/System.State", "value": "Active" },
                { "op": "add", "path": "/fields/WEF_X_Kanban.Column", "value": "Active" },
                { "op": "add", "path": "/fields/WEF_X_Kanban.Column.Done", "value": true }
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 5 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        let board = board(vec![column("Active", true, &[("User Story", "Active")])]);
        let target = board.targets("User Story").unwrap().pop().unwrap();

        client
            .move_work_item("contoso", "MyProject", 5, &board, &target)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn assign_work_item_sets_assigned_to() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/5"))
            .and(body_json(json!([
                { "op": "add", "path": "/fields/System.AssignedTo", "value": "me@example.com" }
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 5 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        client
            .assign_work_item("contoso", "MyProject", 5, "me@example.com")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unassign_clears_assignee_and_moves_in_one_patch() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/5"))
            .and(body_json(json!([
                { "op": "add", "path": "/fields/System.AssignedTo", "value": "" },
                { "op": "add", "path": "/fields/System.State", "value": "New" },
                { "op": "add", "path": "/fields/WEF_X_Kanban.Column", "value": "Ready" },
                { "op": "add", "path": "/fields/WEF_X_Kanban.Column.Done", "value": false }
            ])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 5 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        let board = board(vec![column("Ready", false, &[("User Story", "New")])]);
        let target = board.targets("User Story").unwrap().remove(0);

        client
            .unassign_work_item("contoso", "MyProject", 5, &board, &target)
            .await
            .unwrap();
    }
}
