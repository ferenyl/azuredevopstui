use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::{StreamExt, stream};
use serde::Serialize;
use serde_json::{Value, json};

use super::models::{
    ChangeKind, ChangedFile, Comment, FileContent, GitItem, Identity, IterationChanges,
    ListResponse, Partial, Person, PolicyEvaluation, PullRequest, PullRequestChanges,
    PullRequestChecks, PullRequestDetails, PullRequestIteration, PullRequestStatus, ResourceRef,
    ReviewSignals, Thread,
};
use super::{MAX_CONCURRENT_REQUESTS, ProjectApi, compare_timestamps, is_unauthorized};
use crate::config::{MergeStrategy, OtherPrsFilter};
use crate::images::markdown_images;
use crate::rich_text::RichText;

const POLICY_API_VERSION: &str = "7.1-preview";
const REVIEWER_POLICIES: [&str; 2] = ["Minimum number of reviewers", "Required reviewers"];
const MERGE_STRATEGY_POLICY: &str = "Require a merge strategy";
/// How long the threads of an unchanged PR are reused; new comments do not change the PR itself.
const THREADS_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// Identity that clears `autoCompleteSetBy`.
const NO_IDENTITY: &str = "00000000-0000-0000-0000-000000000000";

/// What happens when a pull request is completed, as Azure DevOps takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOptions {
    pub merge_strategy: MergeStrategy,
    pub delete_source_branch: bool,
    pub transition_work_items: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_commit_message: Option<String>,
    pub bypass_policy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass_reason: Option<String>,
}

/// Changed files listed per PR; one page, larger PRs are cut off.
const MAX_CHANGES: &str = "2000";
/// Larger files are not diffed.
const MAX_DIFF_BYTES: usize = 512 * 1024;

/// Review signals of someone else's PR and what they were computed from.
pub(super) struct CachedReview {
    fingerprint: Fingerprint,
    approved: bool,
    review: ReviewSignals,
    threads_loaded: Instant,
}

/// The parts of a PR its policies and review signals depend on, besides its threads.
#[derive(PartialEq)]
struct Fingerprint {
    commit: Option<String>,
    is_draft: bool,
    reviewers: Vec<(String, i32, Option<bool>)>,
}

impl Fingerprint {
    fn of(pr: &PullRequest) -> Self {
        Self {
            commit: pr
                .last_merge_source_commit
                .as_ref()
                .map(|commit| commit.commit_id.clone()),
            is_draft: pr.is_draft,
            reviewers: pr
                .reviewers
                .iter()
                .map(|reviewer| (reviewer.id.clone(), reviewer.vote, reviewer.is_required))
                .collect(),
        }
    }
}

impl ProjectApi {
    pub async fn my_pull_requests(&self, user_id: &str) -> Result<Vec<PullRequest>> {
        self.active_pull_requests(&[("searchCriteria.creatorId", user_id)])
            .await
    }

    /// Active PRs not created by the user. Without filter entries, all PRs in the project.
    pub async fn other_pull_requests(
        &self,
        user_id: &str,
        filter: &OtherPrsFilter,
    ) -> Result<Partial<PullRequest>> {
        self.other_pull_requests_at(user_id, filter, Instant::now())
            .await
    }

    async fn other_pull_requests_at(
        &self,
        user_id: &str,
        filter: &OtherPrsFilter,
        now: Instant,
    ) -> Result<Partial<PullRequest>> {
        let mut pull_requests = Vec::new();
        if filter.reviewers.is_empty() && filter.creators.is_empty() {
            pull_requests = self.active_pull_requests(&[]).await?;
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
                let id = self.identity_id(name).await?;
                pull_requests.extend(self.active_pull_requests(&[(key, &id)]).await?);
            }
        }
        let pull_requests = without_own(pull_requests, user_id);
        let ids: HashSet<u32> = pull_requests.iter().map(|pr| pr.pull_request_id).collect();
        self.reviews
            .lock()
            .expect("review cache")
            .retain(|id, _| ids.contains(id));
        let results: Vec<Result<(PullRequest, bool)>> = stream::iter(pull_requests)
            .map(|mut pr| async move {
                let complete = match self.review(&pr, user_id, now).await {
                    Ok((approved, review)) => {
                        pr.approved = approved;
                        pr.review = review;
                        true
                    }
                    Err(err) if is_unauthorized(&err) => return Err(err),
                    Err(err) => {
                        tracing::warn!(
                            id = pr.pull_request_id,
                            "PR loaded without review signals: {err:#}"
                        );
                        false
                    }
                };
                Ok((pr, complete))
            })
            .buffered(MAX_CONCURRENT_REQUESTS)
            .collect()
            .await;
        let results = results.into_iter().collect::<Result<Vec<_>>>()?;
        let failed = results.iter().filter(|(_, complete)| !complete).count();
        Ok(Partial {
            items: results.into_iter().map(|(pr, _)| pr).collect(),
            failed,
        })
    }

    pub async fn pull_request_details(&self, pr: &PullRequest) -> Result<PullRequestDetails> {
        let id = pr.pull_request_id.to_string();
        let pr_path = self.pr_path(pr, &id);
        let statuses_path = [&pr_path[..], &["statuses"]].concat();
        let work_items_path = [&pr_path[..], &["workitems"]].concat();
        let (full, threads, statuses, policies, work_items): (
            PullRequest,
            _,
            ListResponse<PullRequestStatus>,
            _,
            ListResponse<ResourceRef>,
        ) = tokio::try_join!(
            self.client.get(&self.client.urls.dev_azure, &pr_path, &[]),
            self.threads(pr),
            self.client
                .get(&self.client.urls.dev_azure, &statuses_path, &[]),
            self.policy_evaluations(pr),
            self.client
                .get(&self.client.urls.dev_azure, &work_items_path, &[]),
        )?;
        let ids: Vec<u32> = work_items
            .value
            .iter()
            .filter_map(|item| item.id.parse().ok())
            .collect();

        Ok(PullRequestDetails {
            description: full
                .description
                .as_deref()
                .map(|description| RichText::from_markdown(&markdown_images(description))),
            threads: visible_threads(threads),
            statuses: latest_statuses(statuses.value),
            policies,
            work_items: self.work_items_batch(&ids).await?,
        })
    }

    /// The threads, checks and work item links behind the markers, without the full PR and items.
    pub async fn pull_request_checks(&self, pr: &PullRequest) -> Result<PullRequestChecks> {
        let id = pr.pull_request_id.to_string();
        let pr_path = self.pr_path(pr, &id);
        let statuses_path = [&pr_path[..], &["statuses"]].concat();
        let work_items_path = [&pr_path[..], &["workitems"]].concat();
        let (threads, statuses, policies, work_items): (
            _,
            ListResponse<PullRequestStatus>,
            _,
            ListResponse<ResourceRef>,
        ) = tokio::try_join!(
            self.threads(pr),
            self.client
                .get(&self.client.urls.dev_azure, &statuses_path, &[]),
            self.policy_evaluations(pr),
            self.client
                .get(&self.client.urls.dev_azure, &work_items_path, &[]),
        )?;
        Ok(PullRequestChecks {
            threads: visible_threads(threads),
            statuses: latest_statuses(statuses.value),
            policies,
            has_work_items: !work_items.value.is_empty(),
        })
    }

    /// A pull request by its repository, in any state.
    pub(super) async fn pull_request(
        &self,
        project: &str,
        repository: &str,
        id: u32,
    ) -> Result<PullRequest> {
        let mut pr: PullRequest = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    project,
                    "_apis",
                    "git",
                    "repositories",
                    repository,
                    "pullRequests",
                    &id.to_string(),
                ],
                &[],
            )
            .await?;
        pr.render_description();
        Ok(pr)
    }

    /// Merge strategies the target branch policies allow.
    pub async fn merge_strategies(&self, pr: &PullRequest) -> Result<Vec<MergeStrategy>> {
        let policies = self.policy_evaluations(pr).await?;
        Ok(allowed_merge_strategies(&policies))
    }

    pub async fn complete_pull_request(
        &self,
        pr: &PullRequest,
        options: &CompletionOptions,
    ) -> Result<()> {
        let commit = pr
            .last_merge_source_commit
            .as_ref()
            .context("pull request has no source commit")?;
        self.update_pull_request(
            pr,
            &json!({
                "status": "completed",
                "lastMergeSourceCommit": { "commitId": commit.commit_id },
                "completionOptions": options
            }),
        )
        .await
    }

    /// Completes the pull request by itself once its policies pass.
    pub async fn set_auto_complete(
        &self,
        pr: &PullRequest,
        user_id: &str,
        options: &CompletionOptions,
    ) -> Result<()> {
        self.update_pull_request(
            pr,
            &json!({
                "autoCompleteSetBy": { "id": user_id },
                "completionOptions": options
            }),
        )
        .await
    }

    pub async fn cancel_auto_complete(&self, pr: &PullRequest) -> Result<()> {
        self.update_pull_request(pr, &json!({ "autoCompleteSetBy": { "id": NO_IDENTITY } }))
            .await
    }

    pub async fn update_pull_request(&self, pr: &PullRequest, body: &Value) -> Result<()> {
        let id = pr.pull_request_id.to_string();
        let _: Value = self
            .client
            .patch(&self.client.urls.dev_azure, &self.pr_path(pr, &id), body)
            .await?;
        Ok(())
    }

    /// Adds the reviewer or updates their entry, e.g. `{ "vote": 10 }`.
    pub async fn put_reviewer(
        &self,
        pr: &PullRequest,
        reviewer_id: &str,
        body: &Value,
    ) -> Result<()> {
        let id = pr.pull_request_id.to_string();
        let path = [&self.pr_path(pr, &id)[..], &["reviewers", reviewer_id]].concat();
        let _: Value = self
            .client
            .put(&self.client.urls.dev_azure, &path, body)
            .await?;
        Ok(())
    }

    pub async fn abandon(&self, pr: &PullRequest) -> Result<()> {
        self.update_pull_request(pr, &json!({ "status": "abandoned" }))
            .await
    }

    pub async fn reactivate(&self, pr: &PullRequest) -> Result<()> {
        self.update_pull_request(pr, &json!({ "status": "active" }))
            .await
    }

    /// Publishes a draft, or turns the pull request back into a draft.
    pub async fn set_draft(&self, pr: &PullRequest, is_draft: bool) -> Result<()> {
        self.update_pull_request(pr, &json!({ "isDraft": is_draft }))
            .await
    }

    pub async fn set_title(&self, pr: &PullRequest, title: &str) -> Result<()> {
        self.update_pull_request(pr, &json!({ "title": title }))
            .await
    }

    /// The description as written; the PR lists cut it off.
    pub async fn pull_request_description(&self, pr: &PullRequest) -> Result<String> {
        let id = pr.pull_request_id.to_string();
        let full: Value = self
            .client
            .get(&self.client.urls.dev_azure, &self.pr_path(pr, &id), &[])
            .await?;
        Ok(full["description"].as_str().unwrap_or_default().to_string())
    }

    pub async fn set_description(&self, pr: &PullRequest, description: &str) -> Result<()> {
        self.update_pull_request(pr, &json!({ "description": description }))
            .await
    }

    /// Adds the person `name` finds, e.g. an email address, as a reviewer without a vote.
    pub async fn add_reviewer(&self, pr: &PullRequest, name: &str) -> Result<()> {
        let id = self.identity_id(name).await?;
        self.put_reviewer(pr, &id, &json!({ "vote": 0 })).await
    }

    pub async fn remove_reviewer(&self, pr: &PullRequest, reviewer_id: &str) -> Result<()> {
        let id = pr.pull_request_id.to_string();
        let path = [&self.pr_path(pr, &id)[..], &["reviewers", reviewer_id]].concat();
        self.client.delete(&self.client.urls.dev_azure, &path).await
    }

    /// Links the pull request to a work item, which then lists it.
    pub async fn link_work_item(&self, pr: &PullRequest, work_item_id: u32) -> Result<()> {
        let artifact = format!(
            "vstfs:///Git/PullRequestId/{}%2F{}%2F{}",
            pr.repository.project.id, pr.repository.id, pr.pull_request_id
        );
        self.patch_work_item(
            work_item_id,
            vec![json!({
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "ArtifactLink",
                    "url": artifact,
                    "attributes": { "name": "Pull Request" }
                }
            })],
        )
        .await
    }

    /// Votes: 10 approved, 5 approved with suggestions, 0 none, -5 waiting for author, -10 rejected.
    pub async fn vote(&self, pr: &PullRequest, user_id: &str, vote: i32) -> Result<()> {
        self.put_reviewer(pr, user_id, &json!({ "vote": vote }))
            .await
    }

    /// Starts a thread that is not tied to a file.
    pub async fn create_thread(&self, pr: &PullRequest, content: &str) -> Result<()> {
        let body = json!({
            "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
            "status": "active"
        });
        self.post_thread(pr, &["threads"], &body).await
    }

    /// Replies to the first comment of a thread.
    pub async fn reply(
        &self,
        pr: &PullRequest,
        thread_id: u32,
        parent_comment_id: u32,
        content: &str,
    ) -> Result<()> {
        let body = json!({
            "parentCommentId": parent_comment_id,
            "content": content,
            "commentType": 1
        });
        let thread_id = thread_id.to_string();
        self.post_thread(pr, &["threads", &thread_id, "comments"], &body)
            .await
    }

    /// `status` is one of [`THREAD_STATUSES`].
    pub async fn set_thread_status(
        &self,
        pr: &PullRequest,
        thread_id: u32,
        status: &str,
    ) -> Result<()> {
        let id = pr.pull_request_id.to_string();
        let thread_id = thread_id.to_string();
        let path = [&self.pr_path(pr, &id)[..], &["threads", &thread_id]].concat();
        let _: Value = self
            .client
            .patch(
                &self.client.urls.dev_azure,
                &path,
                &json!({ "status": status }),
            )
            .await?;
        Ok(())
    }

    async fn post_thread(&self, pr: &PullRequest, tail: &[&str], body: &Value) -> Result<()> {
        let id = pr.pull_request_id.to_string();
        let path = [&self.pr_path(pr, &id)[..], tail].concat();
        let _: Value = self
            .client
            .post_versioned(
                &self.client.urls.dev_azure,
                &path,
                &[],
                body,
                super::API_VERSION,
            )
            .await?;
        Ok(())
    }

    /// Approval and review signals of someone else's PR. Policies are loaded again only when the PR
    /// changed, threads also once they are older than [`THREADS_MAX_AGE`].
    async fn review(
        &self,
        pr: &PullRequest,
        user_id: &str,
        now: Instant,
    ) -> Result<(bool, ReviewSignals)> {
        let fingerprint = Fingerprint::of(pr);
        let cached = self
            .reviews
            .lock()
            .expect("review cache")
            .get(&pr.pull_request_id)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| {
                (
                    cached.approved,
                    cached.review.clone(),
                    cached.threads_loaded,
                )
            });
        let (approved, review) = match cached {
            Some((approved, review, loaded))
                if now.saturating_duration_since(loaded) < THREADS_MAX_AGE =>
            {
                return Ok((approved, review));
            }
            Some((approved, _, _)) => (
                approved,
                review_signals(pr, &self.threads(pr).await?, user_id),
            ),
            None => {
                let (policies, threads) =
                    tokio::try_join!(self.policy_evaluations(pr), self.threads(pr))?;
                (
                    is_approved(pr, &policies),
                    review_signals(pr, &threads, user_id),
                )
            }
        };
        self.reviews.lock().expect("review cache").insert(
            pr.pull_request_id,
            CachedReview {
                fingerprint,
                approved,
                review: review.clone(),
                threads_loaded: now,
            },
        );
        Ok((approved, review))
    }

    /// All threads, including system threads such as votes and pushes.
    async fn threads(&self, pr: &PullRequest) -> Result<Vec<Thread>> {
        let id = pr.pull_request_id.to_string();
        let path = [&self.pr_path(pr, &id)[..], &["threads"]].concat();
        let threads: ListResponse<Thread> = self
            .client
            .get(&self.client.urls.dev_azure, &path, &[])
            .await?;
        Ok(threads.value)
    }

    async fn policy_evaluations(&self, pr: &PullRequest) -> Result<Vec<PolicyEvaluation>> {
        let artifact_id = format!(
            "vstfs:///CodeReview/CodeReviewId/{}/{}",
            pr.repository.project.id, pr.pull_request_id
        );
        let response: ListResponse<PolicyEvaluation> = self
            .client
            .get_versioned(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &pr.repository.project.id,
                    "_apis",
                    "policy",
                    "evaluations",
                ],
                &[("artifactId", artifact_id.as_str())],
                POLICY_API_VERSION,
            )
            .await?;
        Ok(response.value)
    }

    /// Queues the build validations `evaluation_ids` of `pr` again.
    pub async fn requeue_policy_evaluations(
        &self,
        pr: &PullRequest,
        evaluation_ids: &[String],
    ) -> Result<()> {
        for id in evaluation_ids {
            let _: Value = self
                .client
                .patch_versioned(
                    &self.client.urls.dev_azure,
                    &[
                        &self.organization,
                        &pr.repository.project.id,
                        "_apis",
                        "policy",
                        "evaluations",
                        id,
                    ],
                    POLICY_API_VERSION,
                )
                .await?;
        }
        Ok(())
    }

    async fn active_pull_requests(&self, criteria: &[(&str, &str)]) -> Result<Vec<PullRequest>> {
        let mut query = vec![("searchCriteria.status", "active"), ("$top", "500")];
        query.extend_from_slice(criteria);
        let response: ListResponse<PullRequest> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &self.project,
                    "_apis",
                    "git",
                    "pullrequests",
                ],
                &query,
            )
            .await?;
        let mut pull_requests = response.value;
        for pr in &mut pull_requests {
            pr.render_description();
        }
        Ok(pull_requests)
    }

    fn pr_path<'a>(&'a self, pr: &'a PullRequest, id: &'a str) -> [&'a str; 8] {
        [
            &self.organization,
            &pr.repository.project.id,
            "_apis",
            "git",
            "repositories",
            &pr.repository.id,
            "pullRequests",
            id,
        ]
    }

    /// Cached for the session.
    async fn identity_id(&self, name: &str) -> Result<String> {
        let cached = self
            .identities
            .lock()
            .expect("identity cache")
            .get(name)
            .cloned();
        if let Some(id) = cached {
            return Ok(id);
        }
        let id = self
            .find_identities(name)
            .await?
            .into_iter()
            .next()
            .map(|identity| identity.id)
            .with_context(|| format!("identity not found: {name}"))?;
        self.identities
            .lock()
            .expect("identity cache")
            .insert(name.to_string(), id.clone());
        Ok(id)
    }

    /// People whose name or email matches `text`.
    pub async fn search_identities(&self, text: &str) -> Result<Vec<Person>> {
        let identities = self.find_identities(text).await?;
        Ok(identities
            .into_iter()
            .map(|identity| {
                let unique_name = identity
                    .properties
                    .account
                    .map_or_else(|| identity.provider_display_name.clone(), |a| a.value);
                Person {
                    display_name: identity.provider_display_name,
                    unique_name,
                }
            })
            .collect())
    }

    async fn find_identities(&self, text: &str) -> Result<Vec<Identity>> {
        let base = format!("{}/{}", self.client.urls.vssps_dev_azure, self.organization);
        let identities: ListResponse<Identity> = self
            .client
            .get(
                &base,
                &["_apis", "identities"],
                &[("searchFilter", "General"), ("filterValue", text)],
            )
            .await?;
        Ok(identities.value)
    }

    /// The files changed in the latest iteration, compared with the target branch.
    pub async fn pull_request_changes(&self, pr: &PullRequest) -> Result<PullRequestChanges> {
        let id = pr.pull_request_id.to_string();
        let iterations_path = [&self.pr_path(pr, &id)[..], &["iterations"]].concat();
        let iterations: ListResponse<PullRequestIteration> = self
            .client
            .get(&self.client.urls.dev_azure, &iterations_path, &[])
            .await?;
        let latest = iterations
            .value
            .into_iter()
            .max_by_key(|iteration| iteration.id)
            .context("pull request has no iterations")?;
        let iteration = latest.id.to_string();
        let changes_path = [&iterations_path[..], &[&iteration, "changes"]].concat();
        let changes: IterationChanges = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &changes_path,
                &[("$compareTo", "0"), ("$top", MAX_CHANGES)],
            )
            .await?;
        Ok(PullRequestChanges {
            base: latest.common_ref_commit.commit_id,
            head: latest.source_ref_commit.commit_id,
            files: changes
                .change_entries
                .into_iter()
                .filter_map(ChangedFile::from_entry)
                .collect(),
        })
    }

    /// `file` before and after the PR; empty on the side where it does not exist.
    pub async fn file_versions(
        &self,
        pr: &PullRequest,
        changes: &PullRequestChanges,
        file: &ChangedFile,
    ) -> Result<(FileContent, FileContent)> {
        let old_path = file.original_path.as_deref().unwrap_or(&file.path);
        let old = async {
            match file.kind {
                ChangeKind::Added => Ok(FileContent::Text(String::new())),
                _ => self.file_at(pr, old_path, &changes.base).await,
            }
        };
        let new = async {
            match file.kind {
                ChangeKind::Deleted => Ok(FileContent::Text(String::new())),
                _ => self.file_at(pr, &file.path, &changes.head).await,
            }
        };
        tokio::try_join!(old, new)
    }

    async fn file_at(&self, pr: &PullRequest, path: &str, commit: &str) -> Result<FileContent> {
        let item: GitItem = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    &pr.repository.project.id,
                    "_apis",
                    "git",
                    "repositories",
                    &pr.repository.id,
                    "items",
                ],
                &[
                    ("path", path),
                    ("versionDescriptor.version", commit),
                    ("versionDescriptor.versionType", "commit"),
                    ("includeContent", "true"),
                    ("$format", "json"),
                ],
            )
            .await?;
        Ok(file_content(item))
    }
}

/// Binary and too large files are skipped.
fn file_content(item: GitItem) -> FileContent {
    let binary = item
        .content_metadata
        .is_some_and(|metadata| metadata.is_binary)
        || item.content.contains('\0');
    if binary || item.content.len() > MAX_DIFF_BYTES {
        FileContent::Skipped
    } else {
        FileContent::Text(item.content)
    }
}

/// Drops duplicates and the user's own PRs.
fn without_own(mut pull_requests: Vec<PullRequest>, user_id: &str) -> Vec<PullRequest> {
    let mut seen = HashSet::new();
    pull_requests.retain(|pr| seen.insert(pr.pull_request_id) && pr.created_by.id != user_id);
    pull_requests
}

/// Blocking reviewer policies all approved, or without them: someone approved and nobody rejected.
pub fn is_approved(pr: &PullRequest, policies: &[PolicyEvaluation]) -> bool {
    let mut reviewer_policies = policies
        .iter()
        .filter(|policy| policy.configuration.is_blocking && is_reviewer_policy(policy))
        .peekable();
    if reviewer_policies.peek().is_none() {
        return pr.reviewers.iter().any(|reviewer| reviewer.vote >= 5)
            && pr.reviewers.iter().all(|reviewer| reviewer.vote >= 0);
    }
    reviewer_policies.all(|policy| policy.status == "approved")
}

pub fn is_reviewer_policy(policy: &PolicyEvaluation) -> bool {
    REVIEWER_POLICIES.contains(&policy.configuration.kind.display_name.as_str())
}

/// Strategies allowed by every blocking merge strategy policy.
fn allowed_merge_strategies(policies: &[PolicyEvaluation]) -> Vec<MergeStrategy> {
    let limits: Vec<_> = policies
        .iter()
        .filter(|policy| {
            policy.configuration.is_blocking
                && policy.configuration.kind.display_name == MERGE_STRATEGY_POLICY
        })
        .map(|policy| &policy.configuration.settings)
        .collect();
    MergeStrategy::ALL
        .into_iter()
        .filter(|strategy| limits.iter().all(|settings| settings.allows(*strategy)))
        .collect()
}

/// What waits on the user in someone else's PR, from its raw threads.
fn review_signals(pr: &PullRequest, threads: &[Thread], user_id: &str) -> ReviewSignals {
    let is_me = |comment: &Comment| comment.author.id.eq_ignore_ascii_case(user_id);
    let latest = |kind: &str, by_me: bool| {
        threads
            .iter()
            .filter(|thread| thread.kind() == Some(kind))
            .flat_map(|thread| &thread.comments)
            .filter(|comment| !by_me || is_me(comment))
            .filter_map(|comment| comment.published_date.clone())
            .max_by(|a, b| compare_timestamps(a, b))
    };
    let my_vote = pr
        .reviewers
        .iter()
        .find(|reviewer| reviewer.id.eq_ignore_ascii_case(user_id))
        .map(|reviewer| reviewer.vote);
    let mention = format!("@<{}>", user_id.to_lowercase());
    let mut signals = ReviewSignals::default();
    let mut waiting = Vec::new();

    if !pr.is_draft && my_vote == Some(0) {
        signals.needs_vote = true;
        waiting.push(Some(pr.creation_date.clone()));
    }
    let (voted, pushed) = (latest("VoteUpdate", true), latest("RefUpdate", false));
    let pushed_after_vote = match (&pushed, &voted) {
        (Some(pushed), Some(voted)) => compare_timestamps(pushed, voted).is_gt(),
        _ => false,
    };
    if my_vote.is_some_and(|vote| vote != 0) && pushed_after_vote {
        signals.changed_since_vote = true;
        waiting.push(pushed);
    }
    for thread in threads
        .iter()
        .filter(|thread| !thread.is_deleted)
        .filter(|thread| matches!(thread.status.as_deref(), Some("active" | "pending")))
    {
        let comments: Vec<&Comment> = thread
            .comments
            .iter()
            .filter(|comment| {
                !comment.is_deleted && comment.comment_type.as_deref() != Some("system")
            })
            .collect();
        let (Some(first), Some(last)) = (comments.first(), comments.last()) else {
            continue;
        };
        if is_me(first) && !is_me(last) {
            signals.replies += 1;
            waiting.push(last.published_date.clone());
        }
        let last_mine = comments.iter().rposition(|comment| is_me(comment));
        let unanswered = comments.iter().enumerate().find(|(index, comment)| {
            !is_me(comment)
                && last_mine.is_none_or(|mine| mine < *index)
                && comment
                    .content
                    .as_deref()
                    .is_some_and(|content| content.to_lowercase().contains(&mention))
        });
        if let Some((_, comment)) = unanswered {
            signals.mentions += 1;
            waiting.push(comment.published_date.clone());
        }
    }
    signals.since = waiting
        .into_iter()
        .map(|date| date.unwrap_or_else(|| pr.creation_date.clone()))
        .min_by(|a, b| compare_timestamps(a, b));
    signals
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
                comment.text =
                    RichText::from_markdown(comment.content.as_deref().unwrap_or_default());
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
    use wiremock::matchers::{body_json, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::api::models::CommitRef;
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

    async fn mount_empty_threads(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path_regex(r"/pullRequests/\d+/threads$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(server)
            .await;
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

    fn pull_request_with_votes(votes: &[i32]) -> PullRequest {
        let mut pr = pull_request(1, "PR", "other", "2026-10-01T10:00:00Z");
        pr.reviewers = votes
            .iter()
            .map(|vote| {
                serde_json::from_value(json!({ "displayName": "R", "vote": vote })).unwrap()
            })
            .collect();
        pr
    }

    #[test]
    fn without_policies_someone_approving_makes_a_pull_request_approved() {
        assert!(is_approved(&pull_request_with_votes(&[10]), &[]));
        assert!(is_approved(&pull_request_with_votes(&[0, 5]), &[]));
        assert!(!is_approved(&pull_request_with_votes(&[0]), &[]));
    }

    #[test]
    fn without_policies_a_rejection_means_not_approved() {
        assert!(!is_approved(&pull_request_with_votes(&[-10]), &[]));
        assert!(!is_approved(&pull_request_with_votes(&[10, -10]), &[]));
        assert!(!is_approved(&pull_request_with_votes(&[10, -5]), &[]));
    }

    fn pull_request_voted_by(user_id: &str, vote: i32) -> PullRequest {
        let mut pr = pull_request(1, "PR", "other", "2026-10-01T08:00:00Z");
        pr.reviewers = vec![
            serde_json::from_value(json!({ "id": user_id, "displayName": "Me", "vote": vote }))
                .unwrap(),
        ];
        pr
    }

    fn system_thread(kind: &str, author: &str, date: &str) -> Thread {
        thread(json!({
            "status": "closed",
            "properties": { "CodeReviewThreadType": { "$value": kind } },
            "comments": [{
                "author": { "id": author, "displayName": "X" },
                "publishedDate": date,
                "commentType": "system"
            }]
        }))
    }

    #[test]
    fn push_after_vote_is_an_update_even_with_fractional_seconds() {
        let threads = [
            system_thread("VoteUpdate", "me", "2026-10-01T10:00:00Z"),
            system_thread("RefUpdate", "other", "2026-10-01T10:00:00.5Z"),
        ];

        let signals = review_signals(&pull_request_voted_by("me", 10), &threads, "me");

        assert!(signals.changed_since_vote);
        assert_eq!(signals.since.as_deref(), Some("2026-10-01T10:00:00.5Z"));
    }

    #[test]
    fn push_before_vote_is_not_an_update_even_with_fractional_seconds() {
        let threads = [
            system_thread("VoteUpdate", "me", "2026-10-01T10:00:00.5Z"),
            system_thread("RefUpdate", "other", "2026-10-01T10:00:00Z"),
        ];

        let signals = review_signals(&pull_request_voted_by("me", 10), &threads, "me");

        assert!(!signals.changed_since_vote);
        assert_eq!(signals.since, None);
    }

    #[test]
    fn since_is_the_oldest_waiting_date_across_precision() {
        let reply = thread(json!({
            "status": "active",
            "comments": [
                {
                    "author": { "id": "me", "displayName": "Me" },
                    "content": "Please rename this",
                    "publishedDate": "2026-10-01T09:00:00Z",
                    "commentType": "text"
                },
                {
                    "author": { "id": "other", "displayName": "Other" },
                    "content": "Done",
                    "publishedDate": "2026-10-01T10:00:00Z",
                    "commentType": "text"
                }
            ]
        }));
        let threads = [
            reply,
            system_thread("VoteUpdate", "me", "2026-10-01T09:30:00Z"),
            system_thread("RefUpdate", "other", "2026-10-01T10:00:00.5Z"),
        ];

        let signals = review_signals(&pull_request_voted_by("me", 10), &threads, "me");

        assert_eq!(signals.replies, 1);
        assert!(signals.changed_since_vote);
        assert_eq!(signals.since.as_deref(), Some("2026-10-01T10:00:00Z"));
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
        assert_eq!(threads[0].comments[0].text.plain(), "Real");
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
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        mount_empty_threads(&server).await;
        let client = ProjectApi::with_base_url(&server.uri());

        let prs = client
            .other_pull_requests("me", &OtherPrsFilter::default())
            .await
            .unwrap();

        assert_eq!(ids_of_prs(&prs.items), [1]);
        assert_eq!(prs.failed, 0);
    }

    #[tokio::test]
    async fn other_pull_requests_keep_pull_requests_whose_threads_fail() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                pull_request_json(1, "Fine", "other", "2026-10-01T10:00:00Z"),
                pull_request_json(2, "Forbidden", "other", "2026-10-01T10:00:00Z")
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"/pullRequests/2/threads$"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        mount_empty_threads(&server).await;
        let client = ProjectApi::with_base_url(&server.uri());

        let prs = client
            .other_pull_requests("me", &OtherPrsFilter::default())
            .await
            .unwrap();

        assert_eq!(ids_of_prs(&prs.items), [1, 2]);
        assert_eq!(prs.failed, 1);
    }

    #[tokio::test]
    async fn other_pull_requests_propagate_unauthorized_from_threads() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                pull_request_json(1, "Theirs", "other", "2026-10-01T10:00:00Z")
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"/pullRequests/1/threads$"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let err = client
            .other_pull_requests("me", &OtherPrsFilter::default())
            .await
            .unwrap_err();

        assert!(is_unauthorized(&err));
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
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        mount_empty_threads(&server).await;
        let client = ProjectApi::with_base_url(&server.uri());
        let filter = OtherPrsFilter {
            reviewers: vec!["[MyProject]\\Developers".into()],
            creators: Vec::new(),
            ..Default::default()
        };

        client.other_pull_requests("me", &filter).await.unwrap();
        let prs = client.other_pull_requests("me", &filter).await.unwrap();

        assert_eq!(ids_of_prs(&prs.items), [5]);
    }

    async fn mount_pull_requests(server: &MockServer, prs: Value, times: Option<u64>) {
        let mock = Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": prs })));
        match times {
            Some(times) => mock.up_to_n_times(times).mount(server).await,
            None => mock.mount(server).await,
        }
    }

    async fn expect_signal_requests(server: &MockServer, policies: u64, threads: u64) {
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .expect(policies)
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"/pullRequests/\d+/threads$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .expect(threads)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn unchanged_pull_requests_reuse_policies_and_recent_threads() {
        let server = MockServer::start().await;
        let pr = pull_request_json(1, "Theirs", "other", "2026-10-01T10:00:00Z");
        mount_pull_requests(&server, json!([pr]), None).await;
        expect_signal_requests(&server, 1, 2).await;
        let client = ProjectApi::with_base_url(&server.uri());
        let filter = OtherPrsFilter::default();
        let start = Instant::now();

        for later in [Duration::ZERO, Duration::from_secs(60), THREADS_MAX_AGE] {
            let prs = client
                .other_pull_requests_at("me", &filter, start + later)
                .await
                .unwrap();
            assert!(prs.items[0].approved);
        }
    }

    #[tokio::test]
    async fn changed_pull_requests_load_policies_and_threads_again() {
        let server = MockServer::start().await;
        let mut pr = pull_request_json(1, "Theirs", "other", "2026-10-01T10:00:00Z");
        pr["lastMergeSourceCommit"] = json!({ "commitId": "c1" });
        mount_pull_requests(&server, json!([pr.clone()]), Some(1)).await;
        pr["lastMergeSourceCommit"] = json!({ "commitId": "c2" });
        mount_pull_requests(&server, json!([pr]), None).await;
        expect_signal_requests(&server, 2, 2).await;
        let client = ProjectApi::with_base_url(&server.uri());
        let filter = OtherPrsFilter::default();

        for _ in 0..2 {
            client.other_pull_requests("me", &filter).await.unwrap();
        }
    }

    #[tokio::test]
    async fn unknown_filter_identity_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let filter = OtherPrsFilter {
            reviewers: Vec::new(),
            creators: vec!["nobody@example.com".into()],
            ..Default::default()
        };

        let err = client.other_pull_requests("me", &filter).await.unwrap_err();

        assert_eq!(err.to_string(), "identity not found: nobody@example.com");
    }

    #[tokio::test]
    async fn single_pull_request_gets_its_description_rendered() {
        let server = MockServer::start().await;
        let mut full = pull_request_json(7, "PR", "other", "2026-10-01T10:00:00Z");
        full["description"] = json!("Adds **bold** filter");
        Mock::given(method("GET"))
            .and(path(
                "/contoso/MyProject/_apis/git/repositories/repo-id/pullRequests/7",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(full))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let pr = client
            .pull_request("MyProject", "repo-id", 7)
            .await
            .unwrap();

        assert_eq!(
            pr.description_text.map(|text| text.plain()).as_deref(),
            Some("Adds bold filter")
        );
    }

    #[tokio::test]
    async fn pull_request_details_combine_threads_statuses_and_policies() {
        let server = MockServer::start().await;
        let pr_path = "/contoso/project-id/_apis/git/repositories/repo-id/pullRequests/7";
        let mut full = pull_request_json(7, "PR", "other", "2026-10-01T10:00:00Z");
        full["description"] = json!("x".repeat(500));
        Mock::given(method("GET"))
            .and(path(pr_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(full))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{pr_path}/threads")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "status": "active", "comments": [comment("Fix this", "text", false)] }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{pr_path}/workitems")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
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
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
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
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        let details = client.pull_request_details(&pr).await.unwrap();

        assert_eq!(details.description.map(|d| d.plain().len()), Some(500));
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

    #[tokio::test]
    async fn my_pull_requests_filter_by_creator() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/MyProject/_apis/git/pullrequests"))
            .and(query_param("searchCriteria.creatorId", "me"))
            .and(query_param("searchCriteria.status", "active"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                pull_request_json(4, "Mine", "me", "2026-10-01T10:00:00Z")
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let prs = client.my_pull_requests("me").await.unwrap();

        assert_eq!(ids_of_prs(&prs), [4]);
    }

    #[tokio::test]
    async fn merge_strategies_follow_blocking_policy() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/project-id/_apis/policy/evaluations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [{
                "status": "approved",
                "configuration": {
                    "isBlocking": true,
                    "type": { "displayName": "Require a merge strategy" },
                    "settings": { "allowSquash": true }
                }
            }]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        let strategies = client.merge_strategies(&pr).await.unwrap();

        assert_eq!(strategies, [MergeStrategy::Squash]);
    }

    const PR_7: &str = "/contoso/project-id/_apis/git/repositories/repo-id/pullRequests/7";

    fn squash() -> CompletionOptions {
        CompletionOptions {
            merge_strategy: MergeStrategy::Squash,
            delete_source_branch: false,
            transition_work_items: false,
            merge_commit_message: None,
            bypass_policy: false,
            bypass_reason: None,
        }
    }

    async fn expect_patch_of_7(server: &MockServer, body: Value) {
        Mock::given(method("PATCH"))
            .and(path(PR_7))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(server)
            .await;
    }

    #[test]
    fn completion_options_leave_out_the_unset_message_and_reason() {
        let value = serde_json::to_value(squash()).unwrap();

        assert_eq!(
            value,
            json!({
                "mergeStrategy": "squash",
                "deleteSourceBranch": false,
                "transitionWorkItems": false,
                "bypassPolicy": false
            })
        );
    }

    #[test]
    fn completion_options_carry_message_and_bypass() {
        let options = CompletionOptions {
            delete_source_branch: true,
            merge_commit_message: Some("Merged".into()),
            bypass_policy: true,
            bypass_reason: Some("why".into()),
            ..squash()
        };

        let value = serde_json::to_value(options).unwrap();

        assert_eq!(value["deleteSourceBranch"], true);
        assert_eq!(value["mergeCommitMessage"], "Merged");
        assert_eq!(value["bypassPolicy"], true);
        assert_eq!(value["bypassReason"], "why");
    }

    #[tokio::test]
    async fn complete_pull_request_patches_status_and_options() {
        let server = MockServer::start().await;
        expect_patch_of_7(
            &server,
            json!({
                "status": "completed",
                "lastMergeSourceCommit": { "commitId": "abc" },
                "completionOptions": {
                    "mergeStrategy": "squash",
                    "deleteSourceBranch": true,
                    "transitionWorkItems": false,
                    "bypassPolicy": false
                }
            }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let mut pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");
        pr.last_merge_source_commit = Some(CommitRef {
            commit_id: "abc".into(),
        });
        let options = CompletionOptions {
            delete_source_branch: true,
            ..squash()
        };

        client.complete_pull_request(&pr, &options).await.unwrap();
    }

    #[tokio::test]
    async fn set_auto_complete_patches_the_setter_and_options() {
        let server = MockServer::start().await;
        expect_patch_of_7(
            &server,
            json!({
                "autoCompleteSetBy": { "id": "me" },
                "completionOptions": {
                    "mergeStrategy": "squash",
                    "deleteSourceBranch": false,
                    "transitionWorkItems": false,
                    "bypassPolicy": false
                }
            }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client
            .set_auto_complete(&pr, "me", &squash())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancel_auto_complete_patches_the_empty_identity() {
        let server = MockServer::start().await;
        expect_patch_of_7(
            &server,
            json!({ "autoCompleteSetBy": { "id": "00000000-0000-0000-0000-000000000000" } }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client.cancel_auto_complete(&pr).await.unwrap();
    }

    #[tokio::test]
    async fn vote_puts_the_vote_on_the_reviewer() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path(
                "/contoso/project-id/_apis/git/repositories/repo-id/pullRequests/7/reviewers/me",
            ))
            .and(body_json(json!({ "vote": -5 })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "vote": -5 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        client.vote(&pr, "me", -5).await.unwrap();
    }

    #[tokio::test]
    async fn failed_vote_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        assert!(client.vote(&pr, "me", 10).await.is_err());
    }

    const THREADS: &str =
        "/contoso/project-id/_apis/git/repositories/repo-id/pullRequests/7/threads";

    async fn expect_thread_call(server: &MockServer, verb: &str, target: &str, body: Value) {
        Mock::given(method(verb))
            .and(path(target))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 1 })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn create_thread_posts_an_active_thread() {
        let server = MockServer::start().await;
        expect_thread_call(
            &server,
            "POST",
            THREADS,
            json!({
                "comments": [{ "parentCommentId": 0, "content": "Hi", "commentType": 1 }],
                "status": "active"
            }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        client.create_thread(&pr, "Hi").await.unwrap();
    }

    #[tokio::test]
    async fn reply_posts_to_the_thread_under_its_first_comment() {
        let server = MockServer::start().await;
        expect_thread_call(
            &server,
            "POST",
            &format!("{THREADS}/3/comments"),
            json!({ "parentCommentId": 2, "content": "Done", "commentType": 1 }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        client.reply(&pr, 3, 2, "Done").await.unwrap();
    }

    #[tokio::test]
    async fn set_thread_status_patches_the_thread() {
        let server = MockServer::start().await;
        expect_thread_call(
            &server,
            "PATCH",
            &format!("{THREADS}/3"),
            json!({ "status": "fixed" }),
        )
        .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        client.set_thread_status(&pr, 3, "fixed").await.unwrap();
    }

    #[tokio::test]
    async fn failed_thread_call_is_an_error_and_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        assert!(client.create_thread(&pr, "Hi").await.is_err());
    }

    #[tokio::test]
    async fn lifecycle_changes_patch_the_pull_request() {
        let server = MockServer::start().await;
        for body in [
            json!({ "status": "abandoned" }),
            json!({ "status": "active" }),
            json!({ "isDraft": false }),
            json!({ "isDraft": true }),
            json!({ "title": "New title" }),
            json!({ "description": "New\ntext" }),
        ] {
            expect_patch_of_7(&server, body).await;
        }
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client.abandon(&pr).await.unwrap();
        client.reactivate(&pr).await.unwrap();
        client.set_draft(&pr, false).await.unwrap();
        client.set_draft(&pr, true).await.unwrap();
        client.set_title(&pr, "New title").await.unwrap();
        client.set_description(&pr, "New\ntext").await.unwrap();
    }

    #[tokio::test]
    async fn description_is_read_in_full_and_as_written() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(PR_7))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "description": "x".repeat(500) + "\n![a](b)"
            })))
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        let description = client.pull_request_description(&pr).await.unwrap();

        assert_eq!(description, "x".repeat(500) + "\n![a](b)");
    }

    #[tokio::test]
    async fn add_reviewer_looks_up_the_identity_and_puts_it_without_a_vote() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("filterValue", "anna@example.com"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "value": [{ "id": "anna-id" }] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(format!("{PR_7}/reviewers/anna-id")))
            .and(body_json(json!({ "vote": 0 })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "vote": 0 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client.add_reviewer(&pr, "anna@example.com").await.unwrap();
    }

    #[tokio::test]
    async fn search_identities_gives_names_and_accounts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("filterValue", "ann"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": "1", "providerDisplayName": "Anna Berg",
                  "properties": { "Account": { "$value": "anna@example.com" } } },
                { "id": "2", "providerDisplayName": "Annika" }
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());

        let people = client.search_identities("ann").await.unwrap();

        assert_eq!(
            people,
            [
                Person {
                    display_name: "Anna Berg".into(),
                    unique_name: "anna@example.com".into()
                },
                Person {
                    display_name: "Annika".into(),
                    unique_name: "Annika".into()
                }
            ]
        );
    }

    #[tokio::test]
    async fn remove_reviewer_deletes_the_reviewer() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("{PR_7}/reviewers/anna-id")))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client.remove_reviewer(&pr, "anna-id").await.unwrap();
    }

    #[tokio::test]
    async fn link_work_item_adds_an_artifact_link_to_the_work_item() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/contoso/MyProject/_apis/wit/workitems/12"))
            .and(body_json(json!([{
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "ArtifactLink",
                    "url": "vstfs:///Git/PullRequestId/project-id%2Frepo-id%2F7",
                    "attributes": { "name": "Pull Request" }
                }
            }])))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 12 })))
            .expect(1)
            .mount(&server)
            .await;
        let client = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(7, "PR", "me", "2026-10-01T10:00:00Z");

        client.link_work_item(&pr, 12).await.unwrap();
    }

    #[tokio::test]
    async fn complete_pull_request_needs_a_source_commit() {
        let client = ProjectApi::with_base_url("http://127.0.0.1:1");
        let pr = pull_request(7, "PR", "other", "2026-10-01T10:00:00Z");

        let err = client
            .complete_pull_request(&pr, &squash())
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "pull request has no source commit");
    }

    const PR_1: &str = "/contoso/project-id/_apis/git/repositories/repo-id/pullRequests/1";
    const ITEMS: &str = "/contoso/project-id/_apis/git/repositories/repo-id/items";

    #[tokio::test]
    async fn pull_request_changes_list_the_latest_iteration_against_the_base() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("{PR_1}/iterations")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": 2, "sourceRefCommit": { "commitId": "head2" }, "commonRefCommit": { "commitId": "base2" } },
                { "id": 1, "sourceRefCommit": { "commitId": "head1" }, "commonRefCommit": { "commitId": "base1" } }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{PR_1}/iterations/2/changes")))
            .and(query_param("$compareTo", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "changeEntries": [
                { "changeType": "edit", "item": { "path": "/src/a.rs", "gitObjectType": "blob" } }
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let api = ProjectApi::with_base_url(&server.uri());

        let changes = api
            .pull_request_changes(&pull_request(1, "PR", "me", "2026-10-01T10:00:00Z"))
            .await
            .unwrap();

        assert_eq!(
            (changes.base.as_str(), changes.head.as_str()),
            ("base2", "head2")
        );
        assert_eq!(changes.files.len(), 1);
        assert_eq!(changes.files[0].path, "/src/a.rs");
    }

    fn changes() -> PullRequestChanges {
        PullRequestChanges {
            base: "base".into(),
            head: "head".into(),
            files: Vec::new(),
        }
    }

    async fn mount_item(server: &MockServer, file: &str, commit: &str, body: Value) {
        Mock::given(method("GET"))
            .and(path(ITEMS))
            .and(query_param("path", file))
            .and(query_param("versionDescriptor.version", commit))
            .and(query_param("versionDescriptor.versionType", "commit"))
            .and(query_param("includeContent", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn renamed_file_is_read_from_its_old_path_at_the_base() {
        let server = MockServer::start().await;
        mount_item(&server, "/old.rs", "base", json!({ "content": "old\n" })).await;
        mount_item(&server, "/new.rs", "head", json!({ "content": "new\n" })).await;
        let api = ProjectApi::with_base_url(&server.uri());
        let file = ChangedFile {
            kind: ChangeKind::Renamed,
            path: "/new.rs".into(),
            original_path: Some("/old.rs".into()),
        };

        let versions = api
            .file_versions(
                &pull_request(1, "PR", "me", "2026-10-01T10:00:00Z"),
                &changes(),
                &file,
            )
            .await
            .unwrap();

        assert_eq!(
            versions,
            (
                FileContent::Text("old\n".into()),
                FileContent::Text("new\n".into())
            )
        );
    }

    #[tokio::test]
    async fn added_and_deleted_files_are_read_on_one_side_only() {
        let server = MockServer::start().await;
        mount_item(&server, "/added.rs", "head", json!({ "content": "a\n" })).await;
        mount_item(&server, "/deleted.rs", "base", json!({ "content": "d\n" })).await;
        let api = ProjectApi::with_base_url(&server.uri());
        let pr = pull_request(1, "PR", "me", "2026-10-01T10:00:00Z");
        let file = |kind, path: &str| ChangedFile {
            kind,
            path: path.into(),
            original_path: None,
        };

        let added = api
            .file_versions(&pr, &changes(), &file(ChangeKind::Added, "/added.rs"))
            .await
            .unwrap();
        let deleted = api
            .file_versions(&pr, &changes(), &file(ChangeKind::Deleted, "/deleted.rs"))
            .await
            .unwrap();

        let empty = || FileContent::Text(String::new());
        assert_eq!(added, (empty(), FileContent::Text("a\n".into())));
        assert_eq!(deleted, (FileContent::Text("d\n".into()), empty()));
    }

    #[test]
    fn binary_and_too_large_files_are_skipped() {
        let item = |value: Value| file_content(serde_json::from_value(value).unwrap());

        assert_eq!(
            item(json!({ "content": "text" })),
            FileContent::Text("text".into())
        );
        assert_eq!(
            item(json!({ "content": "iVBOR", "contentMetadata": { "isBinary": true } })),
            FileContent::Skipped
        );
        assert_eq!(item(json!({ "content": "a\u{0}b" })), FileContent::Skipped);
        assert_eq!(
            item(json!({ "content": "x".repeat(MAX_DIFF_BYTES + 1) })),
            FileContent::Skipped
        );
    }
}
