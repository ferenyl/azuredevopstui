use std::collections::HashSet;

use anyhow::{Context, Result};

use super::AdoClient;
use super::models::{
    Identity, ListResponse, PolicyEvaluation, PullRequest, PullRequestDetails, PullRequestStatus,
    Thread,
};
use crate::config::OtherPrsFilter;
use crate::images::markdown_images;

const POLICY_API_VERSION: &str = "7.1-preview";

impl AdoClient {
    pub async fn my_pull_requests(
        &self,
        organization: &str,
        project: &str,
        user_id: &str,
    ) -> Result<Vec<PullRequest>> {
        self.active_pull_requests(
            organization,
            project,
            &[("searchCriteria.creatorId", user_id)],
        )
        .await
    }

    /// Active PRs not created by the user. Without filter entries, all PRs in the project.
    pub async fn other_pull_requests(
        &self,
        organization: &str,
        project: &str,
        user_id: &str,
        filter: &OtherPrsFilter,
    ) -> Result<Vec<PullRequest>> {
        let mut pull_requests = Vec::new();
        if filter.reviewers.is_empty() && filter.creators.is_empty() {
            pull_requests = self
                .active_pull_requests(organization, project, &[])
                .await?;
        } else {
            let criteria = filter
                .reviewers
                .iter()
                .map(|name| ("searchCriteria.reviewerId", name))
                .chain(
                    filter
                        .creators
                        .iter()
                        .map(|name| ("searchCriteria.creatorId", name)),
                );
            for (key, name) in criteria {
                let id = self.identity_id(organization, name).await?;
                pull_requests.extend(
                    self.active_pull_requests(organization, project, &[(key, &id)])
                        .await?,
                );
            }
        }
        Ok(without_own(pull_requests, user_id))
    }

    pub async fn pull_request_details(
        &self,
        organization: &str,
        project: &str,
        pr: &PullRequest,
    ) -> Result<PullRequestDetails> {
        let id = pr.pull_request_id.to_string();
        let pr_path = [
            organization,
            project,
            "_apis",
            "git",
            "repositories",
            &pr.repository.id,
            "pullRequests",
            &id,
        ];
        let threads_path = [&pr_path[..], &["threads"]].concat();
        let statuses_path = [&pr_path[..], &["statuses"]].concat();
        let artifact_id = format!(
            "vstfs:///CodeReview/CodeReviewId/{}/{id}",
            pr.repository.project.id
        );
        let policies_path = [organization, project, "_apis", "policy", "evaluations"];
        let policies_query = [("artifactId", artifact_id.as_str())];
        let (threads, statuses, policies): (
            ListResponse<Thread>,
            ListResponse<PullRequestStatus>,
            ListResponse<PolicyEvaluation>,
        ) = tokio::try_join!(
            self.get(&self.urls.dev_azure, &threads_path, &[]),
            self.get(&self.urls.dev_azure, &statuses_path, &[]),
            self.get_versioned(
                &self.urls.dev_azure,
                &policies_path,
                &policies_query,
                POLICY_API_VERSION,
            ),
        )?;

        Ok(PullRequestDetails {
            threads: visible_threads(threads.value),
            statuses: latest_statuses(statuses.value),
            policies: policies.value,
        })
    }

    async fn active_pull_requests(
        &self,
        organization: &str,
        project: &str,
        criteria: &[(&str, &str)],
    ) -> Result<Vec<PullRequest>> {
        let mut query = vec![("searchCriteria.status", "active"), ("$top", "500")];
        query.extend_from_slice(criteria);
        let response: ListResponse<PullRequest> = self
            .get(
                &self.urls.dev_azure,
                &[organization, project, "_apis", "git", "pullrequests"],
                &query,
            )
            .await?;
        let mut pull_requests = response.value;
        for pr in &mut pull_requests {
            pr.description = pr.description.as_deref().map(markdown_images);
        }
        Ok(pull_requests)
    }

    async fn identity_id(&self, organization: &str, name: &str) -> Result<String> {
        let base = format!("{}/{organization}", self.urls.vssps_dev_azure);
        let identities: ListResponse<Identity> = self
            .get(
                &base,
                &["_apis", "identities"],
                &[("searchFilter", "General"), ("filterValue", name)],
            )
            .await?;
        identities
            .value
            .into_iter()
            .next()
            .map(|identity| identity.id)
            .with_context(|| format!("identity not found: {name}"))
    }
}

/// Drops duplicates and the user's own PRs.
fn without_own(mut pull_requests: Vec<PullRequest>, user_id: &str) -> Vec<PullRequest> {
    let mut seen = HashSet::new();
    pull_requests.retain(|pr| seen.insert(pr.pull_request_id) && pr.created_by.id != user_id);
    pull_requests
}

/// Threads with their deleted and system comments removed; empty threads are dropped.
fn visible_threads(threads: Vec<Thread>) -> Vec<Thread> {
    threads
        .into_iter()
        .filter(|thread| !thread.is_deleted)
        .map(|mut thread| {
            thread.comments.retain(|comment| {
                !comment.is_deleted && comment.comment_type.as_deref() != Some("system")
            });
            for comment in &mut thread.comments {
                comment.content = comment.content.as_deref().map(markdown_images);
            }
            thread
        })
        .filter(|thread| !thread.comments.is_empty())
        .collect()
}

/// Statuses are a history; keeps the latest per context.
fn latest_statuses(mut statuses: Vec<PullRequestStatus>) -> Vec<PullRequestStatus> {
    statuses.sort_by_key(|status| std::cmp::Reverse(status.id));
    let mut seen = HashSet::new();
    statuses
        .retain(|status| seen.insert((status.context.name.clone(), status.context.genre.clone())));
    statuses
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::auth::Auth;
    use crate::test_support::{ids_of_prs, pull_request, pull_request_json};

    fn thread(value: Value) -> Thread {
        serde_json::from_value(value).unwrap()
    }

    fn comment(content: &str, comment_type: &str, is_deleted: bool) -> Value {
        json!({
            "author": { "id": "u", "displayName": "Anna" },
            "content": content,
            "commentType": comment_type,
            "isDeleted": is_deleted
        })
    }

    fn status(id: u32, name: &str, state: &str) -> PullRequestStatus {
        serde_json::from_value(json!({
            "id": id,
            "state": state,
            "context": { "name": name, "genre": "ci" }
        }))
        .unwrap()
    }

    #[test]
    fn without_own_drops_duplicates_and_own_pull_requests() {
        let prs = vec![
            pull_request(1, "A", "other", "2026-10-01T10:00:00Z"),
            pull_request(2, "Mine", "me", "2026-10-01T10:00:00Z"),
            pull_request(1, "A again", "other", "2026-10-01T10:00:00Z"),
            pull_request(3, "B", "someone", "2026-10-01T10:00:00Z"),
        ];

        assert_eq!(ids_of_prs(&without_own(prs, "me")), [1, 3]);
    }

    #[test]
    fn visible_threads_drop_system_and_deleted_comments() {
        let threads = vec![
            thread(json!({ "status": "active", "comments": [
                comment("Real", "text", false),
                comment("Pushed commits", "system", false),
                comment("Removed", "text", true)
            ]})),
            thread(json!({ "comments": [comment("Voted", "system", false)] })),
            thread(json!({ "isDeleted": true, "comments": [comment("Gone", "text", false)] })),
        ];

        let threads = visible_threads(threads);

        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].comments.len(), 1);
        assert_eq!(threads[0].comments[0].content.as_deref(), Some("Real"));
    }

    #[test]
    fn latest_statuses_keep_newest_per_context() {
        let statuses = vec![
            status(1, "build", "pending"),
            status(3, "build", "succeeded"),
            status(2, "coverage", "failed"),
        ];

        let statuses = latest_statuses(statuses);

        let states: Vec<_> = statuses
            .iter()
            .map(|s| (s.context.name.as_str(), s.state.as_deref().unwrap()))
            .collect();
        assert_eq!(states, [("build", "succeeded"), ("coverage", "failed")]);
    }

    #[tokio::test]
    async fn other_pull_requests_without_filter_list_all_but_own() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .and(query_param("searchCriteria.status", "active"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                pull_request_json(1, "Theirs", "other", "2026-10-01T10:00:00Z"),
                pull_request_json(2, "Mine", "me", "2026-10-01T10:00:00Z")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let prs = client
            .other_pull_requests("contoso", "MyProject", "me", &OtherPrsFilter::default())
            .await
            .unwrap();

        assert_eq!(ids_of_prs(&prs), [1]);
    }

    #[tokio::test]
    async fn other_pull_requests_resolve_filter_identities() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("filterValue", "[MyProject]\\Developers"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "value": [{ "id": "group-id" }] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .and(query_param("searchCriteria.reviewerId", "group-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                pull_request_json(5, "Review me", "other", "2026-10-01T10:00:00Z")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        let filter = OtherPrsFilter {
            reviewers: vec!["[MyProject]\\Developers".into()],
            creators: Vec::new(),
        };

        let prs = client
            .other_pull_requests("contoso", "MyProject", "me", &filter)
            .await
            .unwrap();

        assert_eq!(ids_of_prs(&prs), [5]);
    }

    #[tokio::test]
    async fn unknown_filter_identity_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        let filter = OtherPrsFilter {
            reviewers: Vec::new(),
            creators: vec!["nobody@example.com".into()],
        };

        let err = client
            .other_pull_requests("contoso", "MyProject", "me", &filter)
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "identity not found: nobody@example.com");
    }

    #[tokio::test]
    async fn pull_request_details_combine_threads_statuses_and_policies() {
        let server = MockServer::start().await;
        let pr_path = "/contoso/MyProject/_apis/git/repositories/repo-id/pullRequests/7";
        Mock::given(method("GET"))
            .and(path(format!("{pr_path}/threads")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "status": "active", "comments": [comment("Fix this", "text", false)] }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{pr_path}/statuses")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": 1, "state": "succeeded", "context": { "name": "build" } }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/policy/evaluations"))
            .and(query_param(
                "artifactId",
                "vstfs:///CodeReview/CodeReviewId/project-id/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [{
                "status": "approved",
                "configuration": {
                    "isBlocking": true,
                    "type": { "displayName": "Build" },
                    "settings": { "displayName": "CI" }
                }
            }]})))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        let details = client
            .pull_request_details("contoso", "MyProject", &pr)
            .await
            .unwrap();

        assert_eq!(details.threads.len(), 1);
        assert_eq!(details.statuses.len(), 1);
        assert_eq!(
            details.policies[0]
                .configuration
                .settings
                .display_name
                .as_deref(),
            Some("CI")
        );
    }
}
