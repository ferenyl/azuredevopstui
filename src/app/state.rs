use super::setup::Selection;
use crate::api::{ColumnTarget, PullRequest, PullRequestDetails, WorkItem, WorkItemDetails};

#[derive(Clone)]
pub enum Detail {
    PullRequest(Box<PullRequest>),
    WorkItem(WorkItem),
}

impl Detail {
    fn id(&self) -> u32 {
        match self {
            Self::PullRequest(pr) => pr.pull_request_id,
            Self::WorkItem(item) => item.id,
        }
    }

    pub fn is_pull_request(&self, id: u32) -> bool {
        matches!(self, Self::PullRequest(_)) && self.id() == id
    }

    pub fn is_work_item(&self, id: u32) -> bool {
        matches!(self, Self::WorkItem(_)) && self.id() == id
    }
}

pub struct ColumnPicker {
    pub work_item_id: u32,
    pub targets: Vec<ColumnTarget>,
    pub selection: Selection,
}

pub enum DetailInfo {
    PullRequest(PullRequestDetails),
    WorkItem(WorkItemDetails),
}

/// `None` means not loaded yet.
#[derive(Default)]
pub struct Data {
    pub my_prs: Option<Vec<PullRequest>>,
    pub other_prs: Option<Vec<PullRequest>>,
    pub my_work_items: Option<Vec<WorkItem>>,
    pub ready_work_items: Option<Vec<WorkItem>>,
    pub sprint_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    MyPrs,
    MyWorkItems,
    OtherPrs,
    ReadyWorkItems,
    Detail,
}

pub const LEFT_PANELS: [Panel; 4] = [
    Panel::MyPrs,
    Panel::MyWorkItems,
    Panel::OtherPrs,
    Panel::ReadyWorkItems,
];

impl Data {
    pub fn len(&self, panel: Panel) -> usize {
        match panel {
            Panel::MyPrs => self.my_prs.as_ref().map_or(0, Vec::len),
            Panel::OtherPrs => self.other_prs.as_ref().map_or(0, Vec::len),
            Panel::MyWorkItems => self.my_work_items.as_ref().map_or(0, Vec::len),
            Panel::ReadyWorkItems => self.ready_work_items.as_ref().map_or(0, Vec::len),
            Panel::Detail => 0,
        }
    }

    /// The freshly loaded version of a shown detail, if it is still listed.
    pub fn find(&self, detail: &Detail) -> Option<Detail> {
        match detail {
            Detail::PullRequest(current) => [&self.my_prs, &self.other_prs]
                .into_iter()
                .flatten()
                .flatten()
                .find(|pr| pr.pull_request_id == current.pull_request_id)
                .map(|pr| Detail::PullRequest(Box::new(pr.clone()))),
            Detail::WorkItem(current) => [&self.my_work_items, &self.ready_work_items]
                .into_iter()
                .flatten()
                .flatten()
                .find(|item| item.id == current.id)
                .cloned()
                .map(Detail::WorkItem),
        }
    }

    pub fn detail(&self, panel: Panel, index: usize) -> Option<Detail> {
        let pr = |prs: &Option<Vec<PullRequest>>| {
            prs.as_ref()?
                .get(index)
                .cloned()
                .map(|pr| Detail::PullRequest(Box::new(pr)))
        };
        let item = |items: &Option<Vec<WorkItem>>| {
            items.as_ref()?.get(index).cloned().map(Detail::WorkItem)
        };
        match panel {
            Panel::MyPrs => pr(&self.my_prs),
            Panel::OtherPrs => pr(&self.other_prs),
            Panel::MyWorkItems => item(&self.my_work_items),
            Panel::ReadyWorkItems => item(&self.ready_work_items),
            Panel::Detail => None,
        }
    }
}

impl Panel {
    /// Position in the left column, `None` for the detail panel.
    pub fn index(self) -> Option<usize> {
        LEFT_PANELS.iter().position(|panel| *panel == self)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::MyPrs => "My PRs",
            Self::MyWorkItems => "My work items",
            Self::OtherPrs => "Others' PRs",
            Self::ReadyWorkItems => "Ready (sprint)",
            Self::Detail => "Details",
        }
    }
}
