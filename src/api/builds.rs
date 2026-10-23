use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use futures::{StreamExt, stream};
use serde::Deserialize;
use serde_json::{Value, json};

use super::models::{Build, ListResponse, Named, TimelineRecord};
use super::{API_VERSION, MAX_CONCURRENT_REQUESTS, ProjectApi, compare_timestamps};

const MAX_BUILDS: &str = "1000";
const BUILD_DAYS: i64 = 14;
const MAX_LOG_LINES: usize = 5000;

/// A stage, job or task of a build timeline, with its nesting level.
#[derive(Debug, Clone)]
pub struct TimelineRow {
    pub depth: usize,
    pub record: TimelineRecord,
}

impl ProjectApi {
    pub async fn build_timeline(&self, id: u32) -> Result<Vec<TimelineRecord>> {
        let id = id.to_string();
        let timeline: TimelineResponse = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                    &id,
                    "timeline",
                ],
                &[],
            )
            .await?;
        Ok(timeline.records)
    }

    /// The last `MAX_LOG_LINES` lines of a log of the build.
    pub async fn build_log(&self, id: u32, log_id: u32) -> Result<String> {
        let (id, log_id) = (id.to_string(), log_id.to_string());
        let log = self
            .client
            .get_text(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                    &id,
                    "logs",
                    &log_id,
                ],
                &[],
            )
            .await?;
        Ok(last_lines(&log, MAX_LOG_LINES))
    }

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
        let builds: Vec<Build> = builds
            .into_iter()
            .filter(|build| is_live(build, &live))
            .collect();
        let mut latest = self
            .latest_builds(&builds, &min_time)
            .await
            .inspect_err(|err| tracing::warn!("failed to load latest builds: {err:#}"))
            .unwrap_or_default();
        let mut builds: Vec<Build> = builds
            .into_iter()
            .map(|build| {
                latest
                    .remove(&(build.definition.id, build.source_branch.clone()))
                    .unwrap_or(build)
            })
            .collect();
        builds.sort_by(|a, b| {
            b.failed()
                .cmp(&a.failed())
                .then_with(|| match (&b.queue_time, &a.queue_time) {
                    (Some(b), Some(a)) => compare_timestamps(b, a),
                    (b, a) => b.is_some().cmp(&a.is_some()),
                })
        });
        Ok(builds)
    }

    /// Queues a run of the pipeline; with `source_version` it builds that commit.
    pub async fn queue_build(
        &self,
        definition_id: u32,
        source_branch: &str,
        source_version: Option<&str>,
    ) -> Result<Build> {
        let mut body = json!({
            "definition": { "id": definition_id },
            "sourceBranch": source_branch,
        });
        if let Some(version) = source_version {
            body["sourceVersion"] = json!(version);
        }
        self.client
            .post_versioned(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                ],
                &[],
                &body,
                API_VERSION,
            )
            .await
    }

    pub async fn cancel_build(&self, id: u32) -> Result<()> {
        let id = id.to_string();
        let _: Value = self
            .client
            .patch(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "build",
                    "builds",
                    &id,
                ],
                &json!({ "status": "cancelling" }),
            )
            .await?;
        Ok(())
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

    /// The newest run per pipeline and branch of the builds' pipelines, by anyone, in one query.
    async fn latest_builds(
        &self,
        builds: &[Build],
        min_time: &str,
    ) -> Result<HashMap<(u32, String), Build>> {
        let definitions: BTreeSet<u32> = builds.iter().map(|build| build.definition.id).collect();
        if definitions.is_empty() {
            return Ok(HashMap::new());
        }
        let definitions: Vec<String> = definitions.iter().map(u32::to_string).collect();
        let definitions = definitions.join(",");
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
                    ("definitions", definitions.as_str()),
                    ("minTime", min_time),
                    ("queryOrder", "queueTimeDescending"),
                    ("$top", MAX_BUILDS),
                ],
            )
            .await?;
        Ok(latest_branch_builds(builds.value)
            .into_iter()
            .map(|build| ((build.definition.id, build.source_branch.clone()), build))
            .collect())
    }
}

#[derive(Deserialize)]
struct TimelineResponse {
    #[serde(default)]
    records: Vec<TimelineRecord>,
}

/// The stages, jobs and tasks in run order, nested by depth. Phases are folded into their
/// stage and checkpoints are left out.
pub fn timeline_rows(records: &[TimelineRecord]) -> Vec<TimelineRow> {
    let known: HashSet<&str> = records.iter().map(|record| record.id.as_str()).collect();
    let roots: Vec<&TimelineRecord> = records
        .iter()
        .filter(|record| {
            record
                .parent_id
                .as_deref()
                .is_none_or(|parent| !known.contains(parent))
        })
        .collect();
    let mut rows = Vec::new();
    push_rows(records, roots, 0, &mut rows);
    rows
}

fn push_rows(
    records: &[TimelineRecord],
    mut siblings: Vec<&TimelineRecord>,
    depth: usize,
    rows: &mut Vec<TimelineRow>,
) {
    siblings.sort_by_key(|record| record.order.unwrap_or(u32::MAX));
    for record in siblings {
        if record.kind.starts_with("Checkpoint") {
            continue;
        }
        let children = records
            .iter()
            .filter(|child| child.parent_id.as_deref() == Some(record.id.as_str()))
            .collect();
        if record.kind == "Phase" {
            push_rows(records, children, depth, rows);
        } else {
            rows.push(TimelineRow {
                depth,
                record: record.clone(),
            });
            push_rows(records, children, depth + 1, rows);
        }
    }
}

/// The last `count` lines of `text`.
fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
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
    use wiremock::matchers::{body_json, header, method, path, query_param};
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
                build_json(2, 2, "refs/heads/gone", "completed", "succeeded"),
                build_json(3, 2, "refs/heads/feature", "completed", "failed")
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
            .and(query_param("definitions", "1,2"))
            .and(query_param("minTime", MIN_TIME))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                build_json(9, 1, "refs/heads/feature", "completed", "succeeded"),
                build_json(8, 1, "refs/heads/other", "completed", "succeeded"),
                build_json(7, 2, "refs/heads/feature", "completed", "succeeded")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let builds = client.my_builds_since("me", min_time()).await.unwrap();

        let ids: Vec<_> = builds.iter().map(|build| build.id).collect();
        assert_eq!(ids, [9, 7]);
    }

    fn record(id: &str, parent: Option<&str>, kind: &str, order: u32) -> TimelineRecord {
        TimelineRecord {
            id: id.into(),
            parent_id: parent.map(String::from),
            kind: kind.into(),
            name: id.into(),
            order: Some(order),
            ..TimelineRecord::default()
        }
    }

    #[test]
    fn timeline_rows_nest_by_parent_in_run_order_and_fold_phases() {
        let records = [
            record("task-b", Some("job"), "Task", 2),
            record("job", Some("phase"), "Job", 1),
            record("stage-2", None, "Stage", 2),
            record("task-a", Some("job"), "Task", 1),
            record("phase", Some("stage-1"), "Phase", 1),
            record("stage-1", None, "Stage", 1),
            record("gate", Some("stage-1"), "Checkpoint", 2),
            record("approval", Some("gate"), "Checkpoint.Approval", 1),
        ];

        let rows = timeline_rows(&records);

        let rows: Vec<_> = rows
            .iter()
            .map(|row| (row.depth, row.record.id.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "stage-1"),
                (1, "job"),
                (2, "task-a"),
                (2, "task-b"),
                (0, "stage-2"),
            ]
        );
    }

    #[test]
    fn timeline_rows_treat_records_with_unknown_parents_as_roots() {
        let records = [record("job", Some("missing"), "Job", 1)];

        let rows = timeline_rows(&records);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].depth, 0);
    }

    #[test]
    fn only_the_last_lines_of_a_log_are_kept() {
        assert_eq!(last_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(last_lines("a\nb", 5), "a\nb");
        assert_eq!(last_lines("", 5), "");
    }

    #[tokio::test]
    async fn timeline_and_log_are_loaded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/build/builds/5/timeline"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "records": [{
                    "id": "t1",
                    "parentId": null,
                    "type": "Task",
                    "name": "npm test",
                    "state": "completed",
                    "result": "failed",
                    "order": 3,
                    "errorCount": 2,
                    "log": { "id": 7 },
                    "issues": [{ "type": "error", "message": "boom" }, { "type": "warning" }]
                }]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/build/builds/5/logs/7"))
            .respond_with(ResponseTemplate::new(200).set_body_string("one\ntwo\n"))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let records = client.build_timeline(5).await.unwrap();
        let log = client.build_log(5, 7).await.unwrap();

        assert_eq!(records.len(), 1);
        assert!(records[0].failed());
        assert_eq!(records[0].error_count, 2);
        assert_eq!(records[0].log.as_ref().map(|log| log.id), Some(7));
        assert_eq!(records[0].issues[0].message.as_deref(), Some("boom"));
        assert_eq!(records[0].issues[1].message, None);
        assert_eq!(log, "one\ntwo");
    }

    #[tokio::test]
    async fn queueing_posts_the_pipeline_branch_and_commit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/build/builds"))
            .and(body_json(json!({
                "definition": { "id": 7 },
                "sourceBranch": "refs/heads/main",
                "sourceVersion": "abc123"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(build_json(
                5,
                7,
                "refs/heads/main",
                "notStarted",
                "",
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/contoso/MyProject/_apis/build/builds"))
            .and(body_json(json!({
                "definition": { "id": 7 },
                "sourceBranch": "refs/heads/release"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(build_json(
                6,
                7,
                "refs/heads/release",
                "notStarted",
                "",
            )))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let same = client
            .queue_build(7, "refs/heads/main", Some("abc123"))
            .await
            .unwrap();
        let other = client
            .queue_build(7, "refs/heads/release", None)
            .await
            .unwrap();

        assert_eq!((same.id, other.id), (5, 6));
    }

    #[tokio::test]
    async fn cancelling_patches_the_status_as_plain_json() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/contoso/MyProject/_apis/build/builds/501"))
            .and(header("content-type", "application/json"))
            .and(body_json(json!({ "status": "cancelling" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 501 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        client.cancel_build(501).await.unwrap();
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
