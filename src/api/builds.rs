use std::collections::HashSet;

use anyhow::Result;
use chrono::{TimeDelta, Utc};
use futures::future;

use super::AdoClient;
use super::models::{Build, ListResponse};

const BUILD_DAYS: i64 = 14;
const MAX_BUILDS: &str = "100";

impl AdoClient {
    /// The latest run per pipeline and branch that the user triggered, newest first.
    /// Pull request runs are left out; they show up in the PR's checks.
    pub async fn my_builds(
        &self,
        organization: &str,
        project: &str,
        user_id: &str,
    ) -> Result<Vec<Build>> {
        let min_time = (Utc::now() - TimeDelta::days(BUILD_DAYS)).to_rfc3339();
        let builds: ListResponse<Build> = self
            .get(
                &self.urls.dev_azure,
                &[organization, project, "_apis", "build", "builds"],
                &[
                    ("requestedFor", user_id),
                    ("queryOrder", "queueTimeDescending"),
                    ("minTime", &min_time),
                    ("$top", MAX_BUILDS),
                ],
            )
            .await?;
        let builds = latest_branch_builds(builds.value);
        Ok(future::join_all(builds.into_iter().map(|build| async {
            if !build.failed() {
                return build;
            }
            self.latest_build(organization, project, &build)
                .await
                .inspect_err(|err| tracing::warn!("failed to load latest build: {err:#}"))
                .ok()
                .flatten()
                .unwrap_or(build)
        }))
        .await)
    }

    /// The newest run of the build's pipeline and branch, by anyone.
    async fn latest_build(
        &self,
        organization: &str,
        project: &str,
        build: &Build,
    ) -> Result<Option<Build>> {
        let definition = build.definition.id.to_string();
        let builds: ListResponse<Build> = self
            .get(
                &self.urls.dev_azure,
                &[organization, project, "_apis", "build", "builds"],
                &[
                    ("definitions", definition.as_str()),
                    ("branchName", build.source_branch.as_str()),
                    ("queryOrder", "queueTimeDescending"),
                    ("$top", "1"),
                ],
            )
            .await?;
        Ok(builds.value.into_iter().next())
    }
}

/// Keeps the first build per pipeline and branch, skipping pull request runs.
fn latest_branch_builds(builds: Vec<Build>) -> Vec<Build> {
    let mut seen = HashSet::new();
    builds
        .into_iter()
        .filter(|build| build.source_branch.starts_with("refs/heads/"))
        .filter(|build| seen.insert((build.definition.id, build.source_branch.clone())))
        .collect()
}
