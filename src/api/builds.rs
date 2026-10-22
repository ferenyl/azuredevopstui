use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use futures::{StreamExt, stream};

use super::models::{Build, ListResponse, Named};
use super::{MAX_CONCURRENT_REQUESTS, ProjectApi};

const MAX_BUILDS: &str = "1000";
const BUILD_DAYS: i64 = 14;

impl ProjectApi {
    /// The latest run per pipeline and branch that the user triggered, failed first, then newest.
    /// Only runs of the last 14 days count; runs on deleted branches and pull request runs are left out.
    pub async fn my_builds(&self, user_id: &str) -> Result<Vec<Build>> {
        self.my_builds_since(user_id, Utc::now() - Duration::days(BUILD_DAYS))
            .await
    }

    async fn my_builds_since(&self, user_id: &str, min_time: DateTime<Utc>) -> Result<Vec<Build>> {
        let min_time = min_time.to_rfc3339();
        let builds: ListResponse<Build> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                ],
                &[
                    ("requestedFor", user_id),
                    ("minTime", min_time.as_str()),
                    ("queryOrder", "queueTimeDescending"),
                    ("$top", MAX_BUILDS),
                ],
            )
            .await?;
        let builds = latest_branch_builds(builds.value);
        let live = self.live_refs(&builds).await;
        let builds = builds.into_iter().filter(|build| is_live(build, &live));
        let mut builds: Vec<Build> = stream::iter(builds)
            .map(|build| async {
                self.latest_build(&build, &min_time)
                    .await
                    .inspect_err(|err| tracing::warn!("failed to load latest build: {err:#}"))
                    .ok()
                    .flatten()
                    .unwrap_or(build)
            })
            .buffered(MAX_CONCURRENT_REQUESTS)
            .collect()
            .await;
        builds.sort_by(|a, b| {
            b.failed()
                .cmp(&a.failed())
                .then_with(|| b.queue_time.cmp(&a.queue_time))
        });
        Ok(builds)
    }

    /// Branches per Azure Repos repository.
    /// Repositories whose refs fail to load are left out.
    async fn live_refs(&self, builds: &[Build]) -> HashMap<String, HashSet<String>> {
        let repositories: HashSet<&str> = builds
            .iter()
            .filter_map(|build| build.repository.as_ref())
            .filter(|repository| repository.kind == "TfsGit")
            .map(|repository| repository.id.as_str())
            .collect();
        let repositories: Vec<String> = repositories.into_iter().map(str::to_string).collect();
        stream::iter(repositories)
            .map(|repository| async move {
                self.repository_refs(&repository)
                    .await
                    .inspect_err(|err| tracing::warn!("failed to load repository refs: {err:#}"))
                    .ok()
                    .map(|refs| (repository, refs))
            })
            .buffered(MAX_CONCURRENT_REQUESTS)
            .filter_map(|entry| async move { entry })
            .collect()
            .await
    }

    async fn repository_refs(&self, repository: &str) -> Result<HashSet<String>> {
        let path = [
            &self.organization,
            &self.project,
            "_apis",
            "git",
            "repositories",
            repository,
        ];
        let refs_path = [&path[..], &["refs"]].concat();
        let heads: ListResponse<Named> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &refs_path,
                &[("filter", "heads/")],
            )
            .await?;
        Ok(heads.value.into_iter().map(|head| head.name).collect())
    }

    /// The newest run of the build's pipeline and branch, by anyone.
    async fn latest_build(&self, build: &Build, min_time: &str) -> Result<Option<Build>> {
        let definition = build.definition.id.to_string();
        let builds: ListResponse<Build> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                ],
                &[
                    ("definitions", definition.as_str()),
                    ("branchName", build.source_branch.as_str()),
                    ("minTime", min_time),
                    ("queryOrder", "queueTimeDescending"),
                    ("$top", "1"),
                ],
            )
            .await?;
        Ok(builds.value.into_iter().next())
    }
}

/// Keeps the first build per pipeline and branch.
fn latest_branch_builds(builds: Vec<Build>) -> Vec<Build> {
    let mut seen = HashSet::new();
    builds
        .into_iter()
        .filter(|build| build.source_branch.starts_with("refs/heads/"))
        .filter(|build| seen.insert((build.definition.id, build.source_branch.clone())))
        .collect()
}

/// Whether the build's branch still exists; unknown repositories count as live.
fn is_live(build: &Build, live: &HashMap<String, HashSet<String>>) -> bool {
    build
        .repository
        .as_ref()
        .and_then(|repository| live.get(&repository.id))
        .is_none_or(|refs| refs.contains(&build.source_branch))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const MIN_TIME: &str = "2026-10-01T00:00:00+00:00";

    fn min_time() -> DateTime<Utc> {
        MIN_TIME.parse().unwrap()
    }

    fn build_json(id: u32, definition: u32, branch: &str, status: &str, result: &str) -> Value {
        json!({
            "id": id,
            "buildNumber": format!("n{id}"),
            "status": status,
            "result": result,
            "definition": { "id": definition, "name": format!("Pipeline {definition}") },
            "sourceBranch": branch,
            "repository": { "id": "repo", "type": "TfsGit" },
            "queueTime": format!("2026-10-0{id}T10:00:00Z")
        })
    }

    #[tokio::test]
    async fn my_builds_use_latest_run_and_skip_deleted_branches() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/build/builds"))
            .and(query_param("requestedFor", "me"))
            .and(query_param("minTime", MIN_TIME))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                build_json(1, 1, "refs/heads/feature", "completed", "failed"),
                build_json(2, 2, "refs/heads/gone", "completed", "succeeded")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/repositories/repo/refs"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "value": [{ "name": "refs/heads/feature" }] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/build/builds"))
            .and(query_param("definitions", "1"))
            .and(query_param("branchName", "refs/heads/feature"))
            .and(query_param("minTime", MIN_TIME))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                build_json(9, 1, "refs/heads/feature", "completed", "succeeded")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let builds = client.my_builds_since("me", min_time()).await.unwrap();

        let ids: Vec<_> = builds.iter().map(|build| build.id).collect();
        assert_eq!(ids, [9]);
    }

    #[test]
    fn pull_request_runs_are_dropped() {
        let builds = [
            build_json(1, 1, "refs/heads/feature", "completed", "failed"),
            build_json(2, 1, "refs/pull/7/merge", "completed", "failed"),
        ]
        .map(|build| serde_json::from_value(build).unwrap());

        let kept = latest_branch_builds(builds.into());

        let ids: Vec<_> = kept.iter().map(|build| build.id).collect();
        assert_eq!(ids, [1]);
    }
}
