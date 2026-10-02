use std::collections::HashSet;

use anyhow::{Context, Result};

use super::models::{
    Identity, ListResponse, PolicyEvaluation, PullRequest, PullRequestDetails, PullRequestStatus,
    Thread,
};
use super::{AdoClient, DEV_AZURE};
use crate::config::OtherPrsFilter;

const VSSPS_DEV_AZURE: &str = "https://vssps.dev.azure.com";
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
            let mut seen = HashSet::new();
            pull_requests.retain(|pr| seen.insert(pr.pull_request_id));
        }
        pull_requests.retain(|pr| pr.created_by.id != user_id);
        Ok(pull_requests)
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
            self.get(DEV_AZURE, &threads_path, &[]),
            self.get(DEV_AZURE, &statuses_path, &[]),
            self.get_versioned(
                DEV_AZURE,
                &policies_path,
                &policies_query,
                POLICY_API_VERSION,
            ),
        )?;

        let threads = threads
            .value
            .into_iter()
            .filter(|thread| !thread.is_deleted)
            .map(|mut thread| {
                thread.comments.retain(|comment| {
                    !comment.is_deleted && comment.comment_type.as_deref() != Some("system")
                });
                thread
            })
            .filter(|thread| !thread.comments.is_empty())
            .collect();

        // Statuses are a history; keep the latest per context.
        let mut statuses = statuses.value;
        statuses.sort_by_key(|status| std::cmp::Reverse(status.id));
        let mut seen = HashSet::new();
        statuses.retain(|status| {
            seen.insert((status.context.name.clone(), status.context.genre.clone()))
        });

        Ok(PullRequestDetails {
            threads,
            statuses,
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
                DEV_AZURE,
                &[organization, project, "_apis", "git", "pullrequests"],
                &query,
            )
            .await?;
        Ok(response.value)
    }

    async fn identity_id(&self, organization: &str, name: &str) -> Result<String> {
        let base = format!("{VSSPS_DEV_AZURE}/{organization}");
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
