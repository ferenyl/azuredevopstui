use anyhow::{Context, Result};

use super::models::{Backlog, ListResponse, Named};
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
        let columns: ListResponse<Named> = self
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
                    "columns",
                ],
                &[],
            )
            .await?;
        Ok(columns.value.into_iter().map(|c| c.name).collect())
    }
}
