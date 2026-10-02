use super::setup::Selection;
use crate::api::{ColumnTarget, PullRequest, PullRequestDetails, WorkItem, WorkItemDetails};
use crate::config::{PrSort, SortConfig, WorkItemSort};

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

    pub fn tabs(&self) -> &'static [DetailTab] {
        match self {
            Self::PullRequest(_) => &[DetailTab::Overview, DetailTab::Comments, DetailTab::Checks],
            Self::WorkItem(_) => &[DetailTab::Overview, DetailTab::Comments],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailTab {
    #[default]
    Overview,
    Comments,
    Checks,
}

impl DetailTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Comments => "Comments",
            Self::Checks => "Checks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    PullRequests,
    WorkItems,
}

pub enum Popup {
    Column(ColumnPicker),
    Sort {
        kind: SortKind,
        selection: Selection,
    },
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

    pub fn sort(&mut self, sort: &SortConfig) {
        for prs in [&mut self.my_prs, &mut self.other_prs]
            .into_iter()
            .flatten()
        {
            sort_pull_requests(prs, sort.pull_requests);
        }
        for items in [&mut self.my_work_items, &mut self.ready_work_items]
            .into_iter()
            .flatten()
        {
            sort_work_items(items, sort.work_items);
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

fn sort_pull_requests(prs: &mut [PullRequest], sort: PrSort) {
    match sort {
        PrSort::Newest => prs.sort_by(|a, b| b.creation_date.cmp(&a.creation_date)),
        PrSort::Oldest => prs.sort_by(|a, b| a.creation_date.cmp(&b.creation_date)),
        PrSort::Title => prs.sort_by_key(|pr| pr.title.to_lowercase()),
        PrSort::Repository => prs.sort_by(|a, b| {
            a.repository
                .name
                .to_lowercase()
                .cmp(&b.repository.name.to_lowercase())
                .then_with(|| b.creation_date.cmp(&a.creation_date))
        }),
    }
}

fn sort_work_items(items: &mut [WorkItem], sort: WorkItemSort) {
    let newest_change =
        |a: &WorkItem, b: &WorkItem| b.fields.changed_date.cmp(&a.fields.changed_date);
    match sort {
        WorkItemSort::Priority => items.sort_by(|a, b| {
            let priority = |item: &WorkItem| item.fields.priority.unwrap_or(u8::MAX);
            priority(a)
                .cmp(&priority(b))
                .then_with(|| newest_change(a, b))
        }),
        WorkItemSort::Changed => items.sort_by(newest_change),
        WorkItemSort::Created => {
            items.sort_by(|a, b| b.fields.created_date.cmp(&a.fields.created_date));
        }
        WorkItemSort::State => items.sort_by(|a, b| {
            a.fields
                .state
                .cmp(&b.fields.state)
                .then_with(|| newest_change(a, b))
        }),
        WorkItemSort::Id => items.sort_by_key(|item| item.id),
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

    pub fn sort_kind(self) -> Option<SortKind> {
        match self {
            Self::MyPrs | Self::OtherPrs => Some(SortKind::PullRequests),
            Self::MyWorkItems | Self::ReadyWorkItems => Some(SortKind::WorkItems),
            Self::Detail => None,
        }
    }
}
