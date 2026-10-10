use std::collections::{HashMap, HashSet};

use super::setup::Selection;
use crate::api::{
    Build, ColumnTarget, Iteration, Mention, PullRequest, PullRequestDetails, WorkItem,
    WorkItemDetails, is_approved, is_reviewer_policy,
};
use crate::config::{MergeStrategy, PrSort, SortConfig, WorkItemSort};

#[derive(Clone)]
pub enum Detail {
    PullRequest(Box<PullRequest>),
    WorkItem(Box<WorkItem>),
    Build(Box<Build>),
}

impl Detail {
    fn id(&self) -> u32 {
        match self {
            Self::PullRequest(pr) => pr.pull_request_id,
            Self::WorkItem(item) => item.id,
            Self::Build(build) => build.id,
        }
    }

    /// Identifies the item across PRs and work items.
    pub fn key(&self) -> String {
        match self {
            Self::PullRequest(pr) => format!("!{}", pr.pull_request_id),
            Self::WorkItem(item) => format!("#{}", item.id),
            Self::Build(build) => format!("build {}", build.id),
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
            Self::Build(_) => &[DetailTab::Overview],
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
    Complete(CompletePicker),
    Tags(TagPicker),
    Sprint(SprintPicker),
    Comment {
        work_item_id: u32,
        text: String,
    },
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

pub struct CompletePicker {
    pub pr: Box<PullRequest>,
    pub strategies: Vec<MergeStrategy>,
    pub selection: Selection,
}

pub struct TagPicker {
    pub work_item_id: u32,
    pub current: Vec<String>,
    /// Existing tags, most recently used first.
    pub tags: Vec<String>,
    pub query: String,
    /// Matching tags on the item, then other matching tags, then the query when it would be a new tag.
    pub selection: Selection,
}

impl TagPicker {
    pub fn new(work_item_id: u32, current: Vec<String>, tags: Vec<String>) -> Self {
        let mut picker = Self {
            work_item_id,
            current,
            tags,
            query: String::new(),
            selection: Selection::new(Vec::new()),
        };
        picker.filter();
        picker
    }

    pub fn filter(&mut self) {
        let query = self.query.trim().to_lowercase();
        let others = self.tags.iter().filter(|tag| !self.has(tag));
        let mut items: Vec<String> = self
            .current
            .iter()
            .chain(others)
            .filter(|tag| tag.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let exists = self.tags.iter().any(|tag| tag.to_lowercase() == query);
        if !query.is_empty() && !exists && !self.has(&query) {
            items.push(self.query.trim().to_string());
        }
        self.selection = Selection::new(items);
    }

    /// Whether `index` is the query offered as a new tag.
    pub fn is_new(&self, index: usize) -> bool {
        self.selection
            .items
            .get(index)
            .is_some_and(|item| !self.tags.contains(item) && !self.has(item))
    }

    /// Whether `index` is a tag already on the item.
    pub fn is_current(&self, index: usize) -> bool {
        self.selection
            .items
            .get(index)
            .is_some_and(|item| self.has(item))
    }

    pub fn has(&self, tag: &str) -> bool {
        self.current
            .iter()
            .any(|current| current.eq_ignore_ascii_case(tag))
    }
}

pub struct SprintPicker {
    /// Furthest in the future first.
    pub sprints: Vec<Iteration>,
    pub query: String,
    /// Paths of the sprints matching the query.
    pub selection: Selection,
}

impl SprintPicker {
    pub fn new(sprints: Vec<Iteration>, selected: Option<&str>) -> Self {
        let mut picker = Self {
            sprints,
            query: String::new(),
            selection: Selection::new(Vec::new()),
        };
        picker.filter();
        let selected = picker.sprints.iter().position(|sprint| match selected {
            Some(path) => sprint.path == path,
            None => sprint.is_current(),
        });
        picker.selection.selected = selected.unwrap_or(0);
        picker
    }

    pub fn filter(&mut self) {
        let query = self.query.trim().to_lowercase();
        let items = self
            .sprints
            .iter()
            .filter(|sprint| sprint.path.to_lowercase().contains(&query))
            .map(|sprint| sprint.path.clone())
            .collect();
        self.selection = Selection::new(items);
    }

    pub fn sprint(&self, path: &str) -> Option<&Iteration> {
        self.sprints.iter().find(|sprint| sprint.path == path)
    }
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
    pub sprint_start: Option<String>,
    pub sprint_finish: Option<String>,
    /// Latest run per pipeline and branch that the user triggered.
    pub builds: Option<Vec<Build>>,
    /// Things to act on in my PRs, by PR id.
    pub pr_signals: HashMap<u32, Signals>,
    /// Work items with unanswered mentions of the user.
    pub mentions: Option<Vec<Mention>>,
    /// Inbox entries already seen; `None` until the first refresh after loading.
    pub inbox_seen: Option<HashSet<String>>,
}

/// Something that waits on the user.
pub struct InboxEntry {
    pub detail: Detail,
    /// When it started waiting.
    pub since: String,
}

/// What needs attention in a pull request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Signals {
    pub unresolved: usize,
    pub waiting: bool,
    pub rejected: bool,
    pub failed_checks: bool,
    pub conflicts: bool,
    /// No work item is linked.
    pub unlinked: bool,
    /// Approved and nothing blocks the merge.
    pub ready: bool,
}

impl Signals {
    pub fn new(pr: &PullRequest, details: &PullRequestDetails) -> Self {
        let failed_policy = details.policies.iter().any(|policy| {
            policy.configuration.is_blocking
                && !is_reviewer_policy(policy)
                && matches!(policy.status.as_str(), "rejected" | "broken")
        });
        let failed_status = details
            .statuses
            .iter()
            .any(|status| matches!(status.state.as_deref(), Some("failed" | "error")));
        let mut signals = Self {
            unresolved: details
                .threads
                .iter()
                .filter(|thread| matches!(thread.status.as_deref(), Some("active" | "pending")))
                .count(),
            waiting: pr.reviewers.iter().any(|reviewer| reviewer.vote == -5),
            rejected: pr.reviewers.iter().any(|reviewer| reviewer.vote == -10),
            failed_checks: failed_policy || failed_status,
            conflicts: matches!(pr.merge_status.as_deref(), Some("conflicts" | "failure")),
            unlinked: false,
            ready: false,
        };
        let blocking_approved = details
            .policies
            .iter()
            .filter(|policy| policy.configuration.is_blocking)
            .all(|policy| policy.status == "approved");
        signals.ready = !pr.is_draft
            && blocking_approved
            && is_approved(pr, &details.policies)
            && signals == Self::default();
        signals.unlinked = details.work_items.is_empty();
        signals
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Inbox,
    MyPrs,
    MyWorkItems,
    OtherPrs,
    ReadyWorkItems,
    Builds,
    Detail,
}

pub const LEFT_PANELS: [Panel; 6] = [
    Panel::Inbox,
    Panel::MyPrs,
    Panel::MyWorkItems,
    Panel::OtherPrs,
    Panel::ReadyWorkItems,
    Panel::Builds,
];

impl Data {
    pub fn len(&self, panel: Panel) -> usize {
        match panel {
            Panel::Inbox => self.inbox().len(),
            Panel::MyPrs => self.my_prs.as_ref().map_or(0, Vec::len),
            Panel::OtherPrs => self.shown_other_prs().map_or(0, |prs| prs.len()),
            Panel::MyWorkItems => self.my_work_items.as_ref().map_or(0, Vec::len),
            Panel::ReadyWorkItems => self.ready_work_items.as_ref().map_or(0, Vec::len),
            Panel::Builds => self.builds.as_ref().map_or(0, Vec::len),
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

    /// Everything waiting on the user, oldest first.
    pub fn inbox(&self) -> Vec<InboxEntry> {
        let pr_entry = |pr: &PullRequest, since: &str| InboxEntry {
            detail: Detail::PullRequest(Box::new(pr.clone())),
            since: since.to_string(),
        };
        let mine = self
            .my_prs
            .iter()
            .flatten()
            .filter(|pr| {
                self.pr_signals
                    .get(&pr.pull_request_id)
                    .is_some_and(|signals| *signals != Signals::default())
            })
            .map(|pr| pr_entry(pr, &pr.creation_date));
        let others = self
            .other_prs
            .iter()
            .flatten()
            .filter_map(|pr| Some(pr_entry(pr, pr.review.since.as_deref()?)));
        let mentions = self.mentions.iter().flatten().map(|mention| InboxEntry {
            detail: Detail::WorkItem(Box::new(mention.item.clone())),
            since: mention.date.clone(),
        });
        let builds = self
            .builds
            .iter()
            .flatten()
            .filter(|build| build.failed())
            .map(|build| InboxEntry {
                detail: Detail::Build(Box::new(build.clone())),
                since: build
                    .finish_time
                    .clone()
                    .or_else(|| build.queue_time.clone())
                    .unwrap_or_default(),
            });
        let mut entries: Vec<InboxEntry> =
            mine.chain(others).chain(mentions).chain(builds).collect();
        entries.sort_by(|a, b| a.since.cmp(&b.since));
        entries
    }

    /// In the inbox since the last look at it.
    pub fn is_new(&self, detail: &Detail) -> bool {
        self.inbox_seen
            .as_ref()
            .is_some_and(|seen| !seen.contains(&detail.key()))
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
                .chain(self.mentions.iter().flatten().map(|mention| &mention.item))
                .find(|item| item.id == current.id)
                .cloned()
                .map(|item| Detail::WorkItem(Box::new(item))),
            Detail::Build(current) => self
                .builds
                .iter()
                .flatten()
                .find(|build| build.id == current.id)
                .map(|build| Detail::Build(Box::new(build.clone()))),
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
            items
                .as_ref()?
                .get(index)
                .cloned()
                .map(|item| Detail::WorkItem(Box::new(item)))
        };
        match panel {
            Panel::Inbox => self
                .inbox()
                .into_iter()
                .nth(index)
                .map(|entry| entry.detail),
            Panel::MyPrs => pr(&self.my_prs),
            Panel::OtherPrs => self
                .shown_other_prs()?
                .get(index)
                .map(|pr| Detail::PullRequest(Box::new((*pr).clone()))),
            Panel::MyWorkItems => item(&self.my_work_items),
            Panel::ReadyWorkItems => item(&self.ready_work_items),
            Panel::Builds => self
                .builds
                .as_ref()?
                .get(index)
                .map(|build| Detail::Build(Box::new(build.clone()))),
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
            Self::Inbox => "Inbox",
            Self::MyPrs => "My PRs",
            Self::MyWorkItems => "My work items",
            Self::OtherPrs => "Others' PRs",
            Self::ReadyWorkItems => "Ready (sprint)",
            Self::Builds => "My pipelines",
            Self::Detail => "Details",
        }
    }

    pub fn sort_kind(self) -> Option<SortKind> {
        match self {
            Self::MyPrs | Self::OtherPrs => Some(SortKind::PullRequests),
            Self::MyWorkItems | Self::ReadyWorkItems => Some(SortKind::WorkItems),
            Self::Inbox | Self::Builds | Self::Detail => None,
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
        let item = Detail::WorkItem(Box::new(work_item(5, "Item", "Bug", "New")));

        assert!(pr.is_pull_request(5));
        assert!(!pr.is_work_item(5));
        assert!(item.is_work_item(5));
        assert!(!item.is_pull_request(5));
    }

    #[test]
    fn tabs_depend_on_detail_kind() {
        let pr = Detail::PullRequest(Box::new(pull_request(5, "PR", "u", "2026-10-01")));
        let item = Detail::WorkItem(Box::new(work_item(5, "Item", "Bug", "New")));

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
        assert_eq!(Panel::MyPrs.index(), Some(1));
        assert_eq!(Panel::ReadyWorkItems.index(), Some(4));
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
            work_items: vec![work_item(9, "Linked", "User Story", "Active")],
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

        assert_eq!(
            Signals::new(&pr, &details),
            Signals {
                ready: true,
                ..Signals::default()
            }
        );
    }

    #[test]
    fn draft_or_unapproved_pull_request_is_not_ready() {
        let mut draft = pull_request(1, "PR", "me", "2026-10-01");
        draft.is_draft = true;
        let mut unapproved = pull_request(2, "PR", "me", "2026-10-01");
        unapproved.reviewers[0].vote = 0;
        let pending_policy = details(json!([]), json!([]), json!([policy("queued", true)]));
        let no_details = details(json!([]), json!([]), json!([]));

        assert!(!Signals::new(&draft, &no_details).ready);
        assert!(!Signals::new(&unapproved, &no_details).ready);
        assert!(!Signals::new(&pull_request(3, "PR", "me", "2026-10-01"), &pending_policy).ready);
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
