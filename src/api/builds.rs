use std::collections::{HashMap, HashSet};

use anyhow::Result;
use futures::future;

use super::AdoClient;
use super::models::{Build, ListResponse, Named, PullRequest};

const MAX_BUILDS: &str = "1000";

impl AdoClient {
    /// The latest run per pipeline and branch that the user triggered, failed first, then newest.
    /// Runs on deleted branches or closed pull requests are left out.
    pub async fn my_builds(
        &self,
        organization: &str,
        project: &str,
        user_id: &str,
    ) -> Result<Vec<Build>> {
        let builds: ListResponse<Build> = self
            .get(
                &self.urls.dev_azure,
                &[organization, project, "_apis", "build", "builds"],
                &[
                    ("requestedFor", user_id),
                    ("queryOrder", "queueTimeDescending"),
                    ("$top", MAX_BUILDS),
                ],
            )
            .await?;
        let builds = latest_branch_builds(builds.value);
        let live = self.live_refs(organization, project, &builds).await;
        let builds = builds.into_iter().filter(|build| is_live(build, &live));
        let mut builds: Vec<Build> = future::join_all(builds.map(|build| async {
            self.latest_build(organization, project, &build)
                .await
                .inspect_err(|err| tracing::warn!("failed to load latest build: {err:#}"))
                .ok()
                .flatten()
                .unwrap_or(build)
        }))
        .await;
        builds.sort_by(|a, b| {
            b.failed()
                .cmp(&a.failed())
                .then_with(|| b.queue_time.cmp(&a.queue_time))
        });
        Ok(builds)
    }

    /// Branches and active pull request merge refs per Azure Repos repository.
    /// Repositories whose refs fail to load are left out.
    async fn live_refs(
        &self,
        organization: &str,
        project: &str,
        builds: &[Build],
    ) -> HashMap<String, HashSet<String>> {
        let repositories: HashSet<&str> = builds
            .iter()
            .filter_map(|build| build.repository.as_ref())
            .filter(|repository| repository.kind == "TfsGit")
            .map(|repository| repository.id.as_str())
            .collect();
        future::join_all(repositories.into_iter().map(|repository| async move {
            self.repository_refs(organization, project, repository)
                .await
                .inspect_err(|err| tracing::warn!("failed to load repository refs: {err:#}"))
                .ok()
                .map(|refs| (repository.to_string(), refs))
        }))
        .await
        .into_iter()
        .flatten()
        .collect()
    }

    async fn repository_refs(
        &self,
        organization: &str,
        project: &str,
        repository: &str,
    ) -> Result<HashSet<String>> {
        let path = [
            organization,
            project,
            "_apis",
            "git",
            "repositories",
            repository,
        ];
        let refs_path = [&path[..], &["refs"]].concat();
        let pull_requests_path = [&path[..], &["pullrequests"]].concat();
        let (heads, pull_requests) = futures::try_join!(
            self.get::<ListResponse<Named>>(
                &self.urls.dev_azure,
                &refs_path,
                &[("filter", "heads/")],
            ),
            self.get::<ListResponse<PullRequest>>(
                &self.urls.dev_azure,
                &pull_requests_path,
                &[("searchCriteria.status", "active"), ("$top", "1000")],
            ),
        )?;
        Ok(heads
            .value
            .into_iter()
            .map(|head| head.name)
            .chain(
                pull_requests
                    .value
                    .into_iter()
                    .map(|pr| format!("refs/pull/{}/merge", pr.pull_request_id)),
            )
            .collect())
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

/// Keeps the first build per pipeline and branch or pull request.
fn latest_branch_builds(builds: Vec<Build>) -> Vec<Build> {
    let mut seen = HashSet::new();
    builds
        .into_iter()
        .filter(|build| {
            build.source_branch.starts_with("refs/heads/")
                || build.source_branch.starts_with("refs/pull/")
        })
        .filter(|build| seen.insert((build.definition.id, build.source_branch.clone())))
        .collect()
}

/// Whether the build's branch or pull request still exists; unknown repositories count as live.
fn is_live(build: &Build, live: &HashMap<String, HashSet<String>>) -> bool {
    build
        .repository
        .as_ref()
        .and_then(|repository| live.get(&repository.id))
        .is_none_or(|refs| refs.contains(&build.source_branch))
}
