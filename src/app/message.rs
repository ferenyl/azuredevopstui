use anyhow::Result;

use crate::api::{
    Board, Build, CurrentUser, Iteration, Mention, Partial, PullRequest, PullRequestChecks,
    PullRequestDetails, SprintWorkItems, WorkItem, WorkItemDetails,
};
use crate::auth::Auth;
use crate::config::MergeStrategy;

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
    BoardColumns(Result<Vec<String>>),
    WorkItemTypes(Result<Vec<String>>),
    Connected(Result<CurrentUser>),
    MyPullRequests(Result<Vec<PullRequest>>),
    OtherPullRequests(Result<Partial<PullRequest>>),
    SprintWorkItems(Result<SprintWorkItems>),
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
    PullRequestCompleted {
        id: u32,
        result: Result<()>,
    },
    /// A notification closed; holds the item URL when its open action was clicked.
    Notified(Option<String>),
}
