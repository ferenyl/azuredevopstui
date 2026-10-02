use std::collections::HashMap;

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
            Self::WorkItem(_) => &[
                DetailTab::Overview,
                DetailTab::Children,
                DetailTab::Comments,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailTab {
    #[default]
    Overview,
    Children,
    Comments,
    Checks,
}

impl DetailTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Children => "Children",
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
    Types {
        selection: Selection,
        checked: Vec<bool>,
    },
    PrFilter {
        selection: Selection,
        checked: Vec<bool>,
    },
}

pub struct ColumnPicker {
    pub work_item_id: u32,
    pub targets: Vec<ColumnTarget>,
    pub selection: Selection,
    /// Also clear the assignee when the column is confirmed.
    pub unassign: bool,
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
    /// Also list others' PRs that are already approved.
    pub show_approved: bool,
    /// Also list others' draft PRs.
    pub show_drafts: bool,
    pub my_work_items: Option<Vec<WorkItem>>,
    pub ready_work_items: Option<Vec<WorkItem>>,
    pub sprint_name: Option<String>,
    /// Things to act on in my PRs, by PR id.
    pub pr_signals: HashMap<u32, Signals>,
}

/// What needs attention in a pull request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Signals {
    pub unresolved: usize,
    pub waiting: bool,
    pub rejected: bool,
    pub failed_checks: bool,
    pub conflicts: bool,
}

impl Signals {
    pub fn new(pr: &PullRequest, details: &PullRequestDetails) -> Self {
        let failed_policy = details.policies.iter().any(|policy| {
            policy.configuration.is_blocking
                && matches!(policy.status.as_str(), "rejected" | "broken")
        });
        let failed_status = details
            .statuses
            .iter()
            .any(|status| matches!(status.state.as_deref(), Some("failed" | "error")));
        Self {
            unresolved: details
                .threads
                .iter()
                .filter(|thread| matches!(thread.status.as_deref(), Some("active" | "pending")))
                .count(),
            waiting: pr.reviewers.iter().any(|reviewer| reviewer.vote == -5),
            rejected: pr.reviewers.iter().any(|reviewer| reviewer.vote == -10),
            failed_checks: failed_policy || failed_status,
            conflicts: matches!(pr.merge_status.as_deref(), Some("conflicts" | "failure")),
        }
    }
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
            Panel::OtherPrs => self.shown_other_prs().map_or(0, |prs| prs.len()),
            Panel::MyWorkItems => self.my_work_items.as_ref().map_or(0, Vec::len),
            Panel::ReadyWorkItems => self.ready_work_items.as_ref().map_or(0, Vec::len),
            Panel::Detail => 0,
        }
    }

    /// Others' PRs as listed, without approved and draft ones unless shown.
    pub fn shown_other_prs(&self) -> Option<Vec<&PullRequest>> {
        self.other_prs.as_ref().map(|prs| {
            prs.iter()
                .filter(|pr| {
                    (self.show_approved || !pr.approved) && (self.show_drafts || !pr.is_draft)
                })
                .collect()
        })
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
            Panel::OtherPrs => self
                .shown_other_prs()?
                .get(index)
                .map(|pr| Detail::PullRequest(Box::new((*pr).clone()))),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        ids_of_items, ids_of_prs, pull_request, pull_request_in, work_item, work_item_with,
    };
    use serde_json::{Value, json};

    fn sorted_prs(sort: PrSort) -> Vec<u32> {
        let mut prs = vec![
            pull_request(1, "beta", "u", "2026-10-02T10:00:00Z"),
            pull_request(2, "Alpha", "u", "2026-10-03T10:00:00Z"),
            pull_request(3, "gamma", "u", "2026-10-01T10:00:00Z"),
        ];
        sort_pull_requests(&mut prs, sort);
        ids_of_prs(&prs)
    }

    fn sorted_items(sort: WorkItemSort) -> Vec<u32> {
        let mut items = vec![
            work_item_with(1, Some(2), "2026-10-01", "2026-09-03"),
            work_item_with(2, None, "2026-10-05", "2026-09-01"),
            work_item_with(3, Some(1), "2026-10-02", "2026-09-02"),
            work_item_with(4, Some(2), "2026-10-03", "2026-09-04"),
        ];
        sort_work_items(&mut items, sort);
        ids_of_items(&items)
    }

    fn data() -> Data {
        Data {
            my_prs: Some(vec![pull_request(1, "Mine", "me", "2026-10-01T10:00:00Z")]),
            other_prs: Some(vec![
                pull_request(2, "Theirs", "u", "2026-10-01T10:00:00Z"),
                pull_request(3, "Newer", "u", "2026-10-02T10:00:00Z"),
            ]),
            my_work_items: Some(vec![work_item(10, "Story", "User Story", "Active")]),
            ready_work_items: None,
            sprint_name: Some("Sprint 1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn pull_requests_sort_newest_first() {
        assert_eq!(sorted_prs(PrSort::Newest), [2, 1, 3]);
    }

    #[test]
    fn pull_requests_sort_oldest_first() {
        assert_eq!(sorted_prs(PrSort::Oldest), [3, 1, 2]);
    }

    #[test]
    fn pull_requests_sort_by_title_ignoring_case() {
        assert_eq!(sorted_prs(PrSort::Title), [2, 1, 3]);
    }

    #[test]
    fn pull_requests_sort_by_repository_then_newest() {
        let mut prs = vec![
            pull_request_in(1, "web", "2026-10-01T10:00:00Z"),
            pull_request_in(2, "Api", "2026-10-01T10:00:00Z"),
            pull_request_in(3, "web", "2026-10-02T10:00:00Z"),
        ];

        sort_pull_requests(&mut prs, PrSort::Repository);

        assert_eq!(ids_of_prs(&prs), [2, 3, 1]);
    }

    #[test]
    fn work_items_sort_by_priority_with_missing_last() {
        assert_eq!(sorted_items(WorkItemSort::Priority), [3, 4, 1, 2]);
    }

    #[test]
    fn work_items_sort_by_changed() {
        assert_eq!(sorted_items(WorkItemSort::Changed), [2, 4, 3, 1]);
    }

    #[test]
    fn work_items_sort_by_created() {
        assert_eq!(sorted_items(WorkItemSort::Created), [4, 1, 3, 2]);
    }

    #[test]
    fn work_items_sort_by_id() {
        assert_eq!(sorted_items(WorkItemSort::Id), [1, 2, 3, 4]);
    }

    #[test]
    fn work_items_sort_by_state_then_changed() {
        let mut items = vec![
            work_item(1, "a", "Bug", "New"),
            work_item(2, "b", "Bug", "Active"),
            work_item(3, "c", "Bug", "Active"),
        ];
        items[2].fields.changed_date = "2026-12-01".into();

        sort_work_items(&mut items, WorkItemSort::State);

        assert_eq!(ids_of_items(&items), [3, 2, 1]);
    }

    #[test]
    fn data_sort_applies_to_all_lists() {
        let mut data = data();

        data.sort(&SortConfig::default());

        assert_eq!(ids_of_prs(data.other_prs.as_ref().unwrap()), [3, 2]);
    }

    #[test]
    fn data_len_counts_loaded_lists() {
        let data = data();

        assert_eq!(data.len(Panel::MyPrs), 1);
        assert_eq!(data.len(Panel::OtherPrs), 2);
        assert_eq!(data.len(Panel::MyWorkItems), 1);
        assert_eq!(data.len(Panel::ReadyWorkItems), 0);
        assert_eq!(data.len(Panel::Detail), 0);
    }

    #[test]
    fn data_detail_picks_row_from_panel() {
        let data = data();

        assert!(
            data.detail(Panel::OtherPrs, 1)
                .is_some_and(|d| d.is_pull_request(3))
        );
        assert!(
            data.detail(Panel::MyWorkItems, 0)
                .is_some_and(|d| d.is_work_item(10))
        );
        assert!(data.detail(Panel::OtherPrs, 5).is_none());
        assert!(data.detail(Panel::ReadyWorkItems, 0).is_none());
    }

    #[test]
    fn data_find_returns_fresh_version() {
        let mut data = data();
        let shown = data.detail(Panel::OtherPrs, 0).unwrap();
        data.other_prs.as_mut().unwrap()[0].title = "Renamed".into();

        let fresh = data.find(&shown);

        assert!(matches!(fresh, Some(Detail::PullRequest(pr)) if pr.title == "Renamed"));
    }

    #[test]
    fn data_find_is_none_when_item_is_gone() {
        let mut data = data();
        let shown = data.detail(Panel::MyWorkItems, 0).unwrap();
        data.my_work_items = Some(Vec::new());

        assert!(data.find(&shown).is_none());
    }

    #[test]
    fn detail_kind_matches_only_same_type_and_id() {
        let pr = Detail::PullRequest(Box::new(pull_request(5, "PR", "u", "2026-10-01")));
        let item = Detail::WorkItem(work_item(5, "Item", "Bug", "New"));

        assert!(pr.is_pull_request(5));
        assert!(!pr.is_work_item(5));
        assert!(item.is_work_item(5));
        assert!(!item.is_pull_request(5));
    }

    #[test]
    fn tabs_depend_on_detail_kind() {
        let pr = Detail::PullRequest(Box::new(pull_request(5, "PR", "u", "2026-10-01")));
        let item = Detail::WorkItem(work_item(5, "Item", "Bug", "New"));

        assert_eq!(
            pr.tabs(),
            [DetailTab::Overview, DetailTab::Comments, DetailTab::Checks]
        );
        assert_eq!(
            item.tabs(),
            [
                DetailTab::Overview,
                DetailTab::Children,
                DetailTab::Comments
            ]
        );
    }

    #[test]
    fn panel_index_and_sort_kind() {
        assert_eq!(Panel::MyPrs.index(), Some(0));
        assert_eq!(Panel::ReadyWorkItems.index(), Some(3));
        assert_eq!(Panel::Detail.index(), None);
        assert_eq!(Panel::OtherPrs.sort_kind(), Some(SortKind::PullRequests));
        assert_eq!(Panel::MyWorkItems.sort_kind(), Some(SortKind::WorkItems));
        assert_eq!(Panel::Detail.sort_kind(), None);
    }

    fn details(threads: Value, statuses: Value, policies: Value) -> PullRequestDetails {
        PullRequestDetails {
            threads: serde_json::from_value(threads).unwrap(),
            statuses: serde_json::from_value(statuses).unwrap(),
            policies: serde_json::from_value(policies).unwrap(),
        }
    }

    fn policy(status: &str, is_blocking: bool) -> Value {
        json!({
            "status": status,
            "configuration": {
                "isBlocking": is_blocking,
                "type": { "displayName": "Build" },
                "settings": {}
            }
        })
    }

    fn thread(status: &str) -> Value {
        json!({ "status": status, "comments": [] })
    }

    #[test]
    fn healthy_pull_request_has_no_signals() {
        let pr = pull_request(1, "PR", "me", "2026-10-01");
        let details = details(
            json!([thread("fixed"), thread("closed")]),
            json!([{ "id": 1, "state": "succeeded", "context": { "name": "ci" } }]),
            json!([policy("approved", true), policy("rejected", false)]),
        );

        assert_eq!(Signals::new(&pr, &details), Signals::default());
    }

    #[test]
    fn active_and_pending_threads_are_unresolved() {
        let pr = pull_request(1, "PR", "me", "2026-10-01");
        let details = details(
            json!([thread("active"), thread("pending"), thread("fixed")]),
            json!([]),
            json!([]),
        );

        assert_eq!(Signals::new(&pr, &details).unresolved, 2);
    }

    #[test]
    fn reviewer_votes_give_waiting_and_rejected() {
        let mut pr = pull_request(1, "PR", "me", "2026-10-01");
        pr.reviewers[0].vote = -5;
        pr.reviewers[1].vote = -10;

        let signals = Signals::new(&pr, &details(json!([]), json!([]), json!([])));

        assert!(signals.waiting);
        assert!(signals.rejected);
    }

    #[test]
    fn blocking_policy_or_failed_status_is_a_failed_check() {
        let pr = pull_request(1, "PR", "me", "2026-10-01");
        let broken_policy = details(json!([]), json!([]), json!([policy("broken", true)]));
        let failed_status = details(
            json!([]),
            json!([{ "id": 1, "state": "error", "context": { "name": "ci" } }]),
            json!([]),
        );

        assert!(Signals::new(&pr, &broken_policy).failed_checks);
        assert!(Signals::new(&pr, &failed_status).failed_checks);
    }

    #[test]
    fn merge_conflicts_are_flagged() {
        let mut pr = pull_request(1, "PR", "me", "2026-10-01");
        pr.merge_status = Some("conflicts".into());

        let signals = Signals::new(&pr, &details(json!([]), json!([]), json!([])));

        assert!(signals.conflicts);
    }
}
