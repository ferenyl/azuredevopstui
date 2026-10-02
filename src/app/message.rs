use anyhow::Result;

use crate::api::{
    Board, CurrentUser, PullRequest, PullRequestDetails, SprintWorkItems, WorkItem, WorkItemDetails,
};
use crate::auth::Auth;

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
    Connected(Result<CurrentUser>),
    MyPullRequests(Result<Vec<PullRequest>>),
    OtherPullRequests(Result<Vec<PullRequest>>),
    SprintWorkItems(Result<SprintWorkItems>),
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
        result: Result<Board>,
    },
    WorkItemUpdated {
        id: u32,
        result: Result<()>,
    },
}
