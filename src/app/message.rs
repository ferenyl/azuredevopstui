use anyhow::Result;

use crate::api::{
    Board, Build, CurrentUser, Mention, PullRequest, PullRequestDetails, SprintWorkItems, WorkItem,
    WorkItemDetails,
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
    OtherPullRequests(Result<Vec<PullRequest>>),
    SprintWorkItems(Result<SprintWorkItems>),
    Mentions(Result<Vec<Mention>>),
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
        result: Result<PullRequestDetails>,
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
}
