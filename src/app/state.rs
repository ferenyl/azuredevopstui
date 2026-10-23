use std::collections::{HashMap, HashSet};

use super::setup::Selection;
use super::text_input::TextInput;
use crate::api::{
    Build, ColumnTarget, CompletionOptions, Iteration, Mention, PullRequest, PullRequestChecks,
    PullRequestDetails, WorkItem, WorkItemDetails, compare_timestamps, is_approved,
    is_reviewer_policy,
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

    pub fn title(&self) -> &str {
        match self {
            Self::PullRequest(pr) => &pr.title,
            Self::WorkItem(item) => &item.fields.title,
            Self::Build(build) => &build.definition.name,
        }
    }

    /// What the list search matches against.
    pub fn search_text(&self) -> String {
        match self {
            Self::PullRequest(pr) => pr_search_text(pr),
            Self::WorkItem(item) => work_item_search_text(item),
            Self::Build(build) => build_search_text(build),
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
            Self::PullRequest(_) => &[
                DetailTab::Overview,
                DetailTab::Comments,
                DetailTab::Checks,
                DetailTab::Files,
            ],
            Self::WorkItem(_) => &[
                DetailTab::Overview,
                DetailTab::Children,
                DetailTab::Links,
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
    Links,
    Comments,
    Checks,
    Files,
}

impl DetailTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Children => "Children",
            Self::Links => "Links",
            Self::Comments => "Comments",
            Self::Checks => "Checks",
            Self::Files => "Files",
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
    Text {
        target: TextTarget,
        text: TextInput,
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
    BuildFilter {
        text: TextInput,
    },
    Vote {
        pr: Box<PullRequest>,
        selection: Selection,
    },
    /// yes (`y`, enter) or no (`n`, esc) before something that is hard to undo.
    Confirm {
        prompt: String,
        action: ConfirmAction,
    },
    ThreadStatus {
        pr: Box<PullRequest>,
        thread_id: u32,
        selection: Selection,
    },
    /// What can be done to the pull request; `selection` lists the labels of `actions`.
    PrActions {
        pr: Box<PullRequest>,
        actions: Vec<PrAction>,
        selection: Selection,
    },
    /// The reviewers of `pr`, in order.
    RemoveReviewer {
        pr: Box<PullRequest>,
        selection: Selection,
    },
    /// The recent errors, newest first, starting at the error with index `first`.
    Errors {
        first: usize,
    },
}

/// An entry of the pull request menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrAction {
    Abandon,
    Reactivate,
    Publish,
    MarkAsDraft,
    EditTitle,
    EditDescription,
    AddReviewer,
    RemoveReviewer,
    LinkWorkItem,
}

impl PrAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Abandon => "Abandon",
            Self::Reactivate => "Reactivate",
            Self::Publish => "Publish (mark as ready)",
            Self::MarkAsDraft => "Mark as draft",
            Self::EditTitle => "Edit title",
            Self::EditDescription => "Edit description",
            Self::AddReviewer => "Add reviewer",
            Self::RemoveReviewer => "Remove reviewer",
            Self::LinkWorkItem => "Link work item",
        }
    }

    /// The entries that make sense for `pr` as it is now.
    pub fn of(pr: &PullRequest) -> Vec<Self> {
        let mut actions = Vec::new();
        match pr.status.as_str() {
            "active" => {
                actions.push(Self::Abandon);
                actions.push(if pr.is_draft {
                    Self::Publish
                } else {
                    Self::MarkAsDraft
                });
            }
            "abandoned" => actions.push(Self::Reactivate),
            _ => {}
        }
        actions.extend([Self::EditTitle, Self::EditDescription, Self::AddReviewer]);
        if !pr.reviewers.is_empty() {
            actions.push(Self::RemoveReviewer);
        }
        actions.push(Self::LinkWorkItem);
        actions
    }
}

/// What a text popup writes to.
pub enum TextTarget {
    WorkItemComment(u32),
    /// A reply to the thread, or a new thread without one.
    PrThread {
        pr: Box<PullRequest>,
        thread_id: Option<u32>,
    },
    PrTitle(Box<PullRequest>),
    PrDescription(Box<PullRequest>),
    /// A name or email address of the new reviewer.
    AddReviewer(Box<PullRequest>),
    /// The id of the work item to link.
    LinkWorkItem(Box<PullRequest>),
}

pub struct ColumnPicker {
    pub work_item_id: u32,
    pub targets: Vec<ColumnTarget>,
    pub selection: Selection,
    /// Also clear the assignee when the column is confirmed.
    pub unassign: bool,
}

/// What a row of the completion popup stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteRow {
    Strategy(usize),
    DeleteSourceBranch,
    TransitionWorkItems,
    AutoComplete,
    BypassPolicy,
    Message,
    CancelAutoComplete,
}

/// Merge strategies, then the completion options, then cancelling an auto-complete that is set.
pub struct CompletePicker {
    pub pr: Box<PullRequest>,
    pub strategies: Vec<MergeStrategy>,
    /// The strategy ticked; confirming on a strategy row picks that row instead.
    pub strategy: usize,
    pub selection: Selection,
    /// Nothing blocks the merge; otherwise it is only possible with auto-complete or a bypass.
    pub ready: bool,
    pub delete_source_branch: bool,
    pub transition_work_items: bool,
    pub auto_complete: bool,
    pub bypass_policy: bool,
    pub message: TextInput,
}

impl CompletePicker {
    const OPTION_ROWS: [CompleteRow; 5] = [
        CompleteRow::DeleteSourceBranch,
        CompleteRow::TransitionWorkItems,
        CompleteRow::AutoComplete,
        CompleteRow::BypassPolicy,
        CompleteRow::Message,
    ];

    pub fn row(&self) -> CompleteRow {
        let index = self.selection.selected;
        match index.checked_sub(self.strategies.len()) {
            None => CompleteRow::Strategy(index),
            Some(option) => Self::OPTION_ROWS
                .get(option)
                .copied()
                .unwrap_or(CompleteRow::CancelAutoComplete),
        }
    }

    /// Ticks or unticks the option row; a strategy row becomes the ticked strategy.
    pub fn toggle(&mut self) {
        match self.row() {
            CompleteRow::Strategy(index) => self.strategy = index,
            CompleteRow::DeleteSourceBranch => {
                self.delete_source_branch = !self.delete_source_branch
            }
            CompleteRow::TransitionWorkItems => {
                self.transition_work_items = !self.transition_work_items
            }
            CompleteRow::AutoComplete => self.auto_complete = !self.auto_complete,
            CompleteRow::BypassPolicy => self.bypass_policy = !self.bypass_policy,
            CompleteRow::Message | CompleteRow::CancelAutoComplete => {}
        }
    }

    pub fn items(&self) -> Vec<String> {
        let tick = |on: bool| if on { "x" } else { " " };
        let mut items: Vec<String> = self
            .strategies
            .iter()
            .enumerate()
            .map(|(index, strategy)| {
                let mark = if index == self.strategy { "•" } else { " " };
                format!("({mark}) {}", strategy.label())
            })
            .collect();
        let (before, after) = self.message.split();
        let message = if self.row() == CompleteRow::Message {
            format!("{before}▏{after}")
        } else {
            self.message.as_str().to_string()
        };
        items.extend([
            format!("[{}] Delete source branch", tick(self.delete_source_branch)),
            format!(
                "[{}] Transition linked work items",
                tick(self.transition_work_items)
            ),
            format!("[{}] Set auto-complete", tick(self.auto_complete)),
            format!("[{}] Bypass branch policies", tick(self.bypass_policy)),
            format!("Message: {message}"),
        ]);
        if self.pr.auto_complete_set_by.is_some() {
            items.push("Cancel auto-complete".into());
        }
        items
    }
}

/// What a confirm popup does once confirmed.
pub enum ConfirmAction {
    Complete {
        pr: Box<PullRequest>,
        options: CompletionOptions,
        auto_complete: bool,
    },
    Abandon(Box<PullRequest>),
    /// Throws away the typed text; declining brings the text popup back.
    DiscardText {
        target: TextTarget,
        text: TextInput,
    },
}

pub struct TagPicker {
    pub work_item_id: u32,
    pub current: Vec<String>,
    /// The tag set being built with `tab`; starts as `current`.
    pub pending: Vec<String>,
    /// Whether `tab` toggled anything; `enter` then saves `pending` instead of the highlighted row.
    pub toggled: bool,
    /// Existing tags, most recently used first.
    pub tags: Vec<String>,
    pub query: TextInput,
    /// Matching tags on the item, then other matching tags, then the query when it would be a new tag.
    pub selection: Selection,
}

impl TagPicker {
    pub fn new(work_item_id: u32, current: Vec<String>, tags: Vec<String>) -> Self {
        let mut picker = Self {
            work_item_id,
            pending: current.clone(),
            toggled: false,
            current,
            tags,
            query: TextInput::default(),
            selection: Selection::new(Vec::new()),
        };
        picker.filter();
        picker
    }

    pub fn filter(&mut self) {
        let query = self.query.as_str().trim().to_lowercase();
        let added = self.pending.iter().filter(|tag| !self.has(tag));
        let others = self
            .tags
            .iter()
            .filter(|tag| !self.has(tag) && !self.is_pending(tag));
        let mut items: Vec<String> = self
            .current
            .iter()
            .chain(added)
            .chain(others)
            .filter(|tag| tag.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let exists = self.tags.iter().any(|tag| tag.to_lowercase() == query);
        if !query.is_empty() && !exists && !self.has(&query) && !self.is_pending(&query) {
            items.push(self.query.as_str().trim().to_string());
        }
        self.selection = Selection::new(items);
    }

    /// Whether `index` is the query offered as a new tag.
    pub fn is_new(&self, index: usize) -> bool {
        self.selection.items.get(index).is_some_and(|item| {
            !self.tags.contains(item) && !self.has(item) && !self.is_pending(item)
        })
    }

    /// Whether `index` is in the tag set being built.
    pub fn is_checked(&self, index: usize) -> bool {
        self.selection
            .items
            .get(index)
            .is_some_and(|item| self.is_pending(item))
    }

    pub fn has(&self, tag: &str) -> bool {
        self.current
            .iter()
            .any(|current| current.eq_ignore_ascii_case(tag))
    }

    /// Whether `pending` is the same set as `current`, ignoring case and order.
    pub fn is_unchanged(&self) -> bool {
        self.pending.len() == self.current.len() && self.pending.iter().all(|tag| self.has(tag))
    }

    pub fn is_pending(&self, tag: &str) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.eq_ignore_ascii_case(tag))
    }

    /// Adds or removes the highlighted row in `pending`; a new tag also clears the query.
    pub fn toggle(&mut self) {
        let Some(tag) = self.selection.items.get(self.selection.selected).cloned() else {
            return;
        };
        self.toggled = true;
        if self.is_pending(&tag) {
            self.pending
                .retain(|pending| !pending.eq_ignore_ascii_case(&tag));
        } else {
            let was_new = self.is_new(self.selection.selected);
            self.pending.push(tag.clone());
            if was_new {
                self.query = TextInput::default();
            }
        }
        let selected = self.selection.selected;
        self.filter();
        let last = self.selection.items.len().saturating_sub(1);
        let moved = self
            .selection
            .items
            .iter()
            .position(|item| item.eq_ignore_ascii_case(&tag));
        self.selection.selected = moved.unwrap_or(selected.min(last));
    }
}

pub struct SprintPicker {
    /// Furthest in the future first.
    pub sprints: Vec<Iteration>,
    pub query: TextInput,
    /// Paths of the sprints matching the query.
    pub selection: Selection,
}

impl SprintPicker {
    pub fn new(sprints: Vec<Iteration>, selected: Option<&str>) -> Self {
        let mut picker = Self {
            sprints,
            query: TextInput::default(),
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
        let query = self.query.as_str().trim().to_lowercase();
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
    WorkItem(Box<WorkItemDetails>),
}

/// `None` means not loaded yet.
#[derive(Default)]
pub struct Data {
    my_prs: Option<Vec<PullRequest>>,
    other_prs: Option<Vec<PullRequest>>,
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
    builds: Option<Vec<Build>>,
    /// Only list builds whose pipeline, branch or number contains this, ignoring case.
    pub build_filter: String,
    /// Search text per left panel, by `Panel::index`; only matching rows are listed.
    pub filters: [String; LEFT_PANELS.len()],
    /// Things to act on in my PRs, by PR id.
    pr_signals: HashMap<u32, Signals>,
    /// Work items with unanswered mentions of the user.
    mentions: Option<Vec<Mention>>,
    /// Inbox entries already seen; `None` until the first refresh after loading.
    pub inbox_seen: Option<HashSet<String>>,
    /// Everything waiting on the user; rebuilt by the setters of its inputs.
    inbox: Vec<InboxEntry>,
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
    pub fn new(pr: &PullRequest, checks: &PullRequestChecks) -> Self {
        let failed_policy = checks.policies.iter().any(|policy| {
            policy.configuration.is_blocking
                && !is_reviewer_policy(policy)
                && matches!(policy.status.as_str(), "rejected" | "broken")
        });
        let failed_status = checks
            .statuses
            .iter()
            .any(|status| matches!(status.state.as_deref(), Some("failed" | "error")));
        let mut signals = Self {
            unresolved: checks
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
        let blocking_approved = checks
            .policies
            .iter()
            .filter(|policy| policy.configuration.is_blocking)
            .all(|policy| policy.status == "approved");
        signals.ready = !pr.is_draft
            && blocking_approved
            && is_approved(pr, &checks.policies)
            && signals == Self::default();
        signals.unlinked = !checks.has_work_items;
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
    pub fn my_prs(&self) -> Option<&[PullRequest]> {
        self.my_prs.as_deref()
    }

    pub fn other_prs(&self) -> Option<&[PullRequest]> {
        self.other_prs.as_deref()
    }

    pub fn pr_signals(&self) -> &HashMap<u32, Signals> {
        &self.pr_signals
    }

    /// Also drops the signals of PRs that are gone.
    pub fn set_my_prs(&mut self, prs: Vec<PullRequest>) {
        self.pr_signals
            .retain(|id, _| prs.iter().any(|pr| pr.pull_request_id == *id));
        self.my_prs = Some(prs);
        self.rebuild_inbox();
    }

    pub fn set_other_prs(&mut self, prs: Vec<PullRequest>) {
        self.other_prs = Some(prs);
        self.rebuild_inbox();
    }

    pub fn set_builds(&mut self, builds: Vec<Build>) {
        self.builds = Some(builds);
        self.rebuild_inbox();
    }

    pub fn set_mentions(&mut self, mentions: Vec<Mention>) {
        self.mentions = Some(mentions);
        self.rebuild_inbox();
    }

    pub fn set_pr_signals(&mut self, id: u32, signals: Signals) {
        self.pr_signals.insert(id, signals);
        self.rebuild_inbox();
    }

    pub fn len(&self, panel: Panel) -> usize {
        match panel {
            Panel::Inbox => self.shown_inbox().len(),
            Panel::MyPrs => self.shown_my_prs().map_or(0, |prs| prs.len()),
            Panel::OtherPrs => self.shown_other_prs().map_or(0, |prs| prs.len()),
            Panel::MyWorkItems => self.shown_my_work_items().map_or(0, |items| items.len()),
            Panel::ReadyWorkItems => self.shown_ready_work_items().map_or(0, |items| items.len()),
            Panel::Builds => self.shown_builds().map_or(0, |builds| builds.len()),
            Panel::Detail => 0,
        }
    }

    /// The lowercased search of `panel`, if any.
    fn search_of(&self, panel: Panel) -> Option<String> {
        let filter = &self.filters[panel.index()?];
        (!filter.is_empty()).then(|| filter.to_lowercase())
    }

    /// The inbox as listed, matching the search.
    pub fn shown_inbox(&self) -> Vec<&InboxEntry> {
        let search = self.search_of(Panel::Inbox);
        self.inbox
            .iter()
            .filter(|entry| matches_search(&search, || entry.detail.search_text()))
            .collect()
    }

    /// My PRs as listed, matching the search.
    pub fn shown_my_prs(&self) -> Option<Vec<&PullRequest>> {
        let search = self.search_of(Panel::MyPrs);
        self.my_prs.as_ref().map(|prs| {
            prs.iter()
                .filter(|pr| matches_search(&search, || pr_search_text(pr)))
                .collect()
        })
    }

    /// Others' PRs as listed, without approved and draft ones unless shown, matching the search.
    pub fn shown_other_prs(&self) -> Option<Vec<&PullRequest>> {
        let search = self.search_of(Panel::OtherPrs);
        self.other_prs.as_ref().map(|prs| {
            prs.iter()
                .filter(|pr| {
                    (self.show_approved || !pr.approved)
                        && (self.show_drafts || !pr.is_draft)
                        && matches_search(&search, || pr_search_text(pr))
                })
                .collect()
        })
    }

    /// My work items as listed, matching the search.
    pub fn shown_my_work_items(&self) -> Option<Vec<&WorkItem>> {
        self.shown_work_items(Panel::MyWorkItems, &self.my_work_items)
    }

    /// The sprint's ready work items as listed, matching the search.
    pub fn shown_ready_work_items(&self) -> Option<Vec<&WorkItem>> {
        self.shown_work_items(Panel::ReadyWorkItems, &self.ready_work_items)
    }

    fn shown_work_items<'a>(
        &self,
        panel: Panel,
        items: &'a Option<Vec<WorkItem>>,
    ) -> Option<Vec<&'a WorkItem>> {
        let search = self.search_of(panel);
        items.as_ref().map(|items| {
            items
                .iter()
                .filter(|item| matches_search(&search, || work_item_search_text(item)))
                .collect()
        })
    }

    /// Builds as listed, matching the build filter and the search.
    pub fn shown_builds(&self) -> Option<Vec<&Build>> {
        let filter = self.build_filter.to_lowercase();
        let search = self.search_of(Panel::Builds);
        self.builds.as_ref().map(|builds| {
            builds
                .iter()
                .filter(|build| {
                    [
                        &build.definition.name,
                        &build.source_branch,
                        &build.build_number,
                    ]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&filter))
                        && matches_search(&search, || build_search_text(build))
                })
                .collect()
        })
    }

    /// Everything waiting on the user, oldest first.
    pub fn inbox(&self) -> &[InboxEntry] {
        &self.inbox
    }

    fn rebuild_inbox(&mut self) {
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
        entries.sort_by(|a, b| compare_timestamps(&a.since, &b.since));
        self.inbox = entries;
    }

    /// What waits in an inbox entry, like `2 unresolved, checks failed`; empty when nothing to tell.
    pub fn status(&self, detail: &Detail) -> String {
        let Detail::PullRequest(pr) = detail else {
            return String::new();
        };
        let mut parts = Vec::new();
        if let Some(signals) = self.pr_signals.get(&pr.pull_request_id) {
            if signals.unresolved > 0 {
                parts.push(format!("{} unresolved", signals.unresolved));
            }
            for (on, text) in [
                (signals.waiting, "waiting for author"),
                (signals.rejected, "rejected"),
                (signals.failed_checks, "checks failed"),
                (signals.conflicts, "merge conflicts"),
                (signals.unlinked, "no linked work item"),
                (signals.ready, "ready to merge"),
            ] {
                if on {
                    parts.push(text.to_string());
                }
            }
        }
        let review = &pr.review;
        if review.needs_vote {
            parts.push("needs your vote".into());
        }
        if review.changed_since_vote {
            parts.push("updated since your vote".into());
        }
        if review.replies > 0 {
            parts.push(format!("{} replies", review.replies));
        }
        if review.mentions > 0 {
            parts.push(format!("{} mentions", review.mentions));
        }
        parts.join(", ")
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
        self.rebuild_inbox();
    }

    pub fn detail(&self, panel: Panel, index: usize) -> Option<Detail> {
        let pr = |prs: Vec<&PullRequest>| {
            prs.get(index)
                .map(|pr| Detail::PullRequest(Box::new((*pr).clone())))
        };
        let item = |items: Vec<&WorkItem>| {
            items
                .get(index)
                .map(|item| Detail::WorkItem(Box::new((*item).clone())))
        };
        match panel {
            Panel::Inbox => self
                .shown_inbox()
                .get(index)
                .map(|entry| entry.detail.clone()),
            Panel::MyPrs => pr(self.shown_my_prs()?),
            Panel::OtherPrs => pr(self.shown_other_prs()?),
            Panel::MyWorkItems => item(self.shown_my_work_items()?),
            Panel::ReadyWorkItems => item(self.shown_ready_work_items()?),
            Panel::Builds => self
                .shown_builds()?
                .get(index)
                .map(|build| Detail::Build(Box::new((*build).clone()))),
            Panel::Detail => None,
        }
    }
}

fn pr_search_text(pr: &PullRequest) -> String {
    format!(
        "!{} {} {} {}",
        pr.pull_request_id, pr.title, pr.repository.name, pr.created_by.display_name
    )
}

fn work_item_search_text(item: &WorkItem) -> String {
    format!(
        "#{} {} {} {}",
        item.id, item.fields.title, item.fields.work_item_type, item.fields.state
    )
}

fn build_search_text(build: &Build) -> String {
    format!(
        "{} {} {}",
        build.definition.name, build.source_branch, build.build_number
    )
}

/// `search` is lowercased; no search matches everything.
fn matches_search(search: &Option<String>, text: impl FnOnce() -> String) -> bool {
    search
        .as_deref()
        .is_none_or(|search| text().to_lowercase().contains(search))
}

fn sort_pull_requests(prs: &mut [PullRequest], sort: PrSort) {
    match sort {
        PrSort::Newest => {
            prs.sort_by(|a, b| compare_timestamps(&b.creation_date, &a.creation_date));
        }
        PrSort::Oldest => {
            prs.sort_by(|a, b| compare_timestamps(&a.creation_date, &b.creation_date));
        }
        PrSort::Title => prs.sort_by_key(|pr| pr.title.to_lowercase()),
        PrSort::Repository => prs.sort_by(|a, b| {
            a.repository
                .name
                .to_lowercase()
                .cmp(&b.repository.name.to_lowercase())
                .then_with(|| compare_timestamps(&b.creation_date, &a.creation_date))
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

    fn search(data: &mut Data, panel: Panel, text: &str) {
        data.filters[panel.index().unwrap()] = text.into();
    }

    #[test]
    fn search_narrows_every_list_and_len_detail_and_shown_agree() {
        let mut data = data();
        data.ready_work_items = Some(vec![work_item(11, "Other story", "Bug", "New")]);
        search(&mut data, Panel::MyPrs, "mine");
        search(&mut data, Panel::OtherPrs, "NEWER");
        search(&mut data, Panel::MyWorkItems, "#10 sTOry");
        search(&mut data, Panel::ReadyWorkItems, "bug");
        search(&mut data, Panel::Inbox, "nothing");

        assert_eq!(data.len(Panel::MyPrs), 1);
        assert_eq!(data.len(Panel::OtherPrs), 1);
        assert_eq!(data.len(Panel::MyWorkItems), 1);
        assert_eq!(data.len(Panel::ReadyWorkItems), 1);
        assert_eq!(data.len(Panel::Inbox), 0);
        assert!(
            matches!(data.detail(Panel::OtherPrs, 0), Some(Detail::PullRequest(pr)) if pr.pull_request_id == 3)
        );
        assert!(data.detail(Panel::OtherPrs, 1).is_none());
        assert_eq!(data.shown_my_prs().unwrap().len(), data.len(Panel::MyPrs));
    }

    #[test]
    fn search_matches_the_number_and_work_item_type_and_state() {
        let mut data = data();

        search(&mut data, Panel::OtherPrs, "!2");
        assert_eq!(data.len(Panel::OtherPrs), 1);
        search(&mut data, Panel::MyWorkItems, "user story");
        assert_eq!(data.len(Panel::MyWorkItems), 1);
        search(&mut data, Panel::MyWorkItems, "closed");
        assert_eq!(data.len(Panel::MyWorkItems), 0);
        search(&mut data, Panel::MyWorkItems, "");
        assert_eq!(data.len(Panel::MyWorkItems), 1);
    }

    #[test]
    fn search_on_builds_combines_with_the_pipeline_filter() {
        let build = |id: u32, name: &str| -> Build {
            serde_json::from_value(serde_json::json!({
                "id": id, "buildNumber": id.to_string(), "status": "completed",
                "definition": { "id": id, "name": name }, "sourceBranch": "refs/heads/main"
            }))
            .unwrap()
        };
        let mut data = Data::default();
        data.set_builds(vec![
            build(1, "api-ci"),
            build(2, "web-ci"),
            build(3, "web-cd"),
        ]);

        search(&mut data, Panel::Builds, "web");
        assert_eq!(data.len(Panel::Builds), 2);
        data.build_filter = "cd".into();
        assert_eq!(data.len(Panel::Builds), 1);
        assert!(matches!(data.detail(Panel::Builds, 0), Some(Detail::Build(b)) if b.id == 3));
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
    fn pull_requests_sort_by_creation_across_precision_and_offsets() {
        let created = |id, date: &str| pull_request(id, "PR", "u", date);
        let mut prs = vec![
            created(1, "2026-10-01T10:00:00Z"),
            created(2, "2026-10-01T10:00:00.5Z"),
            created(3, "2026-10-01T12:00:00.25+02:00"),
            created(4, "2026-10-01T10:00:00.1Z"),
        ];

        sort_pull_requests(&mut prs, PrSort::Newest);
        assert_eq!(ids_of_prs(&prs), [2, 3, 4, 1]);

        sort_pull_requests(&mut prs, PrSort::Oldest);
        assert_eq!(ids_of_prs(&prs), [1, 4, 3, 2]);
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
            [
                DetailTab::Overview,
                DetailTab::Comments,
                DetailTab::Checks,
                DetailTab::Files
            ]
        );
        assert_eq!(
            item.tabs(),
            [
                DetailTab::Overview,
                DetailTab::Children,
                DetailTab::Links,
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

    fn details(threads: Value, statuses: Value, policies: Value) -> PullRequestChecks {
        PullRequestChecks {
            threads: serde_json::from_value(threads).unwrap(),
            statuses: serde_json::from_value(statuses).unwrap(),
            policies: serde_json::from_value(policies).unwrap(),
            has_work_items: true,
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

    fn failed_build(id: u32) -> Build {
        serde_json::from_value(json!({
            "id": id,
            "buildNumber": "1",
            "status": "completed",
            "result": "failed",
            "definition": { "id": 1, "name": "ci" },
            "sourceBranch": "refs/heads/main",
            "finishTime": "2026-10-01T10:00:00Z",
        }))
        .unwrap()
    }

    fn inbox_keys(data: &Data) -> Vec<String> {
        data.inbox().iter().map(|e| e.detail.key()).collect()
    }

    #[test]
    fn inbox_is_empty_before_anything_is_loaded() {
        assert!(Data::default().inbox().is_empty());
    }

    #[test]
    fn setting_my_prs_and_signals_rebuilds_inbox() {
        let mut data = Data::default();
        data.set_my_prs(vec![pull_request(1, "Mine", "me", "2026-10-01T10:00:00Z")]);
        assert!(data.inbox().is_empty());

        data.set_pr_signals(
            1,
            Signals {
                waiting: true,
                ..Signals::default()
            },
        );

        assert_eq!(data.inbox().len(), 1);
        assert_eq!(data.len(Panel::Inbox), 1);
        assert!(
            data.detail(Panel::Inbox, 0)
                .is_some_and(|d| d.is_pull_request(1))
        );
    }

    #[test]
    fn setting_my_prs_drops_signals_and_entries_of_gone_prs() {
        let mut data = Data::default();
        data.set_my_prs(vec![pull_request(1, "Mine", "me", "2026-10-01T10:00:00Z")]);
        data.set_pr_signals(
            1,
            Signals {
                waiting: true,
                ..Signals::default()
            },
        );

        data.set_my_prs(vec![pull_request(2, "Other", "me", "2026-10-01T10:00:00Z")]);

        assert!(data.pr_signals().is_empty());
        assert!(data.inbox().is_empty());
    }

    #[test]
    fn setting_other_prs_rebuilds_inbox() {
        let mut data = Data::default();
        let mut pr = pull_request(2, "Theirs", "u", "2026-10-01T10:00:00Z");
        pr.review.since = Some("2026-10-02T10:00:00Z".into());

        data.set_other_prs(vec![pr]);

        assert_eq!(data.inbox().len(), 1);
        assert_eq!(data.inbox()[0].since, "2026-10-02T10:00:00Z");
    }

    #[test]
    fn setting_mentions_rebuilds_inbox() {
        let mut data = Data::default();

        data.set_mentions(vec![Mention {
            item: work_item(20, "Mentioned", "Bug", "Active"),
            date: "2026-10-01T10:00:00Z".into(),
        }]);

        assert_eq!(data.inbox().len(), 1);
        data.set_mentions(Vec::new());
        assert!(data.inbox().is_empty());
    }

    #[test]
    fn setting_builds_rebuilds_inbox_with_failed_builds_only() {
        let mut data = Data::default();
        let mut passed = failed_build(2);
        passed.result = Some("succeeded".into());

        data.set_builds(vec![failed_build(1), passed]);

        assert_eq!(
            inbox_keys(&data),
            [Detail::Build(Box::new(failed_build(1))).key()]
        );
    }

    #[test]
    fn inbox_is_sorted_oldest_first_across_sources() {
        let mut data = Data::default();
        data.set_builds(vec![failed_build(1)]);
        data.set_mentions(vec![Mention {
            item: work_item(20, "Mentioned", "Bug", "Active"),
            date: "2026-09-01T10:00:00Z".into(),
        }]);

        let since: Vec<&str> = data.inbox().iter().map(|e| e.since.as_str()).collect();

        assert_eq!(since, ["2026-09-01T10:00:00Z", "2026-10-01T10:00:00Z"]);
    }

    #[test]
    fn inbox_sorts_mixed_precision_and_offsets_by_instant() {
        let mut data = Data::default();
        let mention = |id, date: &str| Mention {
            item: work_item(id, "Mentioned", "Bug", "Active"),
            date: date.into(),
        };
        data.set_mentions(vec![
            mention(20, "2026-10-01T10:00:00Z"),
            mention(21, "2026-10-01T10:00:00.5Z"),
            mention(22, "2026-10-01T12:00:00.1+02:00"),
            mention(23, "2026-10-01T10:00:00.25Z"),
        ]);

        let since: Vec<&str> = data.inbox().iter().map(|e| e.since.as_str()).collect();

        assert_eq!(
            since,
            [
                "2026-10-01T10:00:00Z",
                "2026-10-01T12:00:00.1+02:00",
                "2026-10-01T10:00:00.25Z",
                "2026-10-01T10:00:00.5Z",
            ]
        );
    }
}
