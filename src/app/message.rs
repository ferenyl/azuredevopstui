use anyhow::Result;

use crate::api::{
    Board, Build, CommentsPage, CurrentUser, Iteration, Mention, Partial, PullRequest,
    PullRequestChanges, PullRequestChecks, PullRequestDetails, SprintWorkItems, TimelineRecord,
    WorkItem, WorkItemDetails,
};
use crate::auth::Auth;
use crate::config::{Config, MergeStrategy};
use crate::diff::FileDiff;

use super::loads::Load;

pub enum Message {
    Authenticated(Result<Option<Auth>>),
    Organizations(Result<Vec<String>>),
    Projects {
        organization: String,
        result: Result<Vec<String>>,
    },
    Teams {
        organization: String,
        project: String,
        result: Result<Vec<String>>,
    },
    BoardColumns {
        config: Box<Config>,
        result: Result<Vec<String>>,
    },
    WorkItemTypes(Result<Vec<String>>),
    Connected(Result<CurrentUser>),
    MyPullRequests(Result<Vec<PullRequest>>),
    OtherPullRequests(Result<Partial<PullRequest>>),
    SprintWorkItems(Result<SprintWorkItems>),
    SprintBoard(Result<Vec<WorkItem>>),
    Sprints(Result<Vec<Iteration>>),
    Mentions(Result<Partial<Mention>>),
    Builds(Result<Vec<Build>>),
    TeamBoard(Result<Board>),
    PullRequestDetails {
        id: u32,
        result: Result<PullRequestDetails>,
    },
    WorkItemDetails {
        id: u32,
        result: Result<WorkItemDetails>,
    },
    OlderComments {
        id: u32,
        result: Result<CommentsPage>,
    },
    Board {
        item: WorkItem,
        unassign: bool,
        result: Result<Board>,
    },
    PullRequestSignals {
        id: u32,
        result: Result<PullRequestChecks>,
    },
    Image {
        url: String,
        result: Result<image::DynamicImage>,
    },
    WorkItemUpdated {
        id: u32,
        /// The panels showing what the change touched.
        reload: &'static [Load],
        result: Result<()>,
    },
    Tags {
        item: WorkItem,
        result: Result<Vec<String>>,
    },
    MergeStrategies {
        pr: Box<PullRequest>,
        result: Result<Vec<MergeStrategy>>,
    },
    PullRequestChanges {
        id: u32,
        result: Result<PullRequestChanges>,
    },
    FileDiff {
        id: u32,
        /// Index into the changed files.
        file: usize,
        result: Result<FileDiff>,
    },
    BuildTimeline {
        id: u32,
        result: Result<Vec<TimelineRecord>>,
    },
    BuildLog {
        build_id: u32,
        log_id: u32,
        result: Result<String>,
    },
    PullRequestUpdated {
        id: u32,
        /// The panels showing what the change touched.
        reload: Vec<Load>,
        result: Result<()>,
    },
    /// A pipeline run was queued or cancelled.
    BuildUpdated {
        id: u32,
        result: Result<()>,
    },
    /// The full description of `pr`, which the lists cut off.
    PrDescription {
        pr: Box<PullRequest>,
        result: Result<String>,
    },
    /// A notification closed; holds the item URL when its open action was clicked.
    Notified(Option<String>),
    /// The result of a change started with `App::spawn_action`.
    ActionDone {
        id: u64,
        message: Box<Message>,
    },
    /// The response to a request started with `App::spawn_load`.
    Loaded {
        load: Load,
        generation: u64,
        message: Box<Message>,
    },
}
