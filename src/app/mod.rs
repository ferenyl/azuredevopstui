mod action;
mod message;
mod setup;
mod state;

pub use action::{Action, HELP};
pub use setup::{Retry, Selection, SetupStep};
pub use state::{
    ColumnPicker, CompletePicker, Data, Detail, DetailInfo, DetailTab, InboxEntry, LEFT_PANELS,
    Panel, Popup, Signals, SortKind, TagPicker,
};

use std::time::{Duration, Instant};

use crossterm::clipboard::CopyToClipboard;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::Rect;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::api::{self, AdoClient, Board, CurrentUser, PullRequest, WorkItem};
use crate::auth::Auth;
use crate::config::{AuthMethod, Config, MergeStrategy, PrSort, WorkItemSort};
use crate::images::{Images, image_urls};
use crate::rich_text::RichText;
use crate::theme::Theme;
use message::Message;
use setup::organization_name;

const MAX_RECENT_TAGS: usize = 20;
const NOTICE_DURATION: Duration = Duration::from_secs(3);

pub enum Screen {
    Setup(SetupStep),
    Main,
}

pub struct App {
    pub config: Option<Config>,
    pub theme: Theme,
    pub screen: Screen,
    pub focus: Panel,
    pub user: Option<CurrentUser>,
    pub data: Data,
    pub selections: [usize; LEFT_PANELS.len()],
    pub detail: Option<Detail>,
    /// `None` while the selected detail is loading.
    pub detail_info: Option<DetailInfo>,
    pub detail_scroll: u16,
    pub detail_tab: DetailTab,
    pub popup: Option<Popup>,
    pub show_help: bool,
    pub last_updated: Option<Instant>,
    last_refresh: Instant,
    board: Option<Board>,
    last_left: Panel,
    pub error: Option<String>,
    /// Short confirmation shown in the toolbar until it expires.
    notice: Option<(String, Instant)>,
    pub images: Images,
    client: Option<AdoClient>,
    tx: UnboundedSender<Message>,
    rx: UnboundedReceiver<Message>,
    should_quit: bool,
}

impl App {
    pub fn new(config: Option<Config>, picker: Option<ratatui_image::picker::Picker>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            theme: config
                .as_ref()
                .map(|c| c.colors.clone())
                .unwrap_or_default(),
            config,
            screen: Screen::Setup(SetupStep::Loading("Signing in…")),
            focus: Panel::Inbox,
            user: None,
            data: Data::default(),
            selections: [0; LEFT_PANELS.len()],
            detail: None,
            detail_info: None,
            detail_scroll: 0,
            detail_tab: DetailTab::default(),
            popup: None,
            show_help: false,
            last_updated: None,
            last_refresh: Instant::now(),
            board: None,
            last_left: Panel::Inbox,
            error: None,
            notice: None,
            images: Images::new(picker),
            client: None,
            tx,
            rx,
            should_quit: false,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        self.authenticate();
        let mut events = EventStream::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        while !self.should_quit {
            let frame = terminal.draw(|frame| crate::ui::render(frame, &self))?;
            let stale = stale_cells(frame.buffer, &self.images.stale_areas());
            if !stale.is_empty() {
                let backend = terminal.backend_mut();
                backend.draw(stale.iter().map(|(x, y, cell)| (*x, *y, cell)))?;
                backend.flush()?;
            }
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(err.into()),
                    None => break,
                },
                Some(message) = self.rx.recv() => self.handle_message(message),
                _ = ticker.tick() => self.auto_refresh(),
            }
        }
        Ok(())
    }

    pub fn actions(&self) -> &'static [Action] {
        match &self.screen {
            Screen::Setup(step) if step.is_input() => &[Action::Confirm, Action::Cancel],
            Screen::Setup(SetupStep::Failed {
                retry: Retry::Authenticate,
                ..
            }) if self.can_enter_pat() => &[Action::Reload, Action::ChangeToken, Action::Quit],
            Screen::Setup(SetupStep::Failed { .. }) => &[Action::Reload, Action::Quit],
            Screen::Setup(step) if step.selection().is_some() => {
                &[Action::Down, Action::Up, Action::Confirm, Action::Quit]
            }
            Screen::Setup(_) => &[Action::Quit],
            Screen::Main if self.show_help => &[Action::Cancel],
            Screen::Main
                if matches!(
                    self.popup,
                    Some(Popup::Types { .. } | Popup::PrFilter { .. })
                ) =>
            {
                &[
                    Action::Down,
                    Action::Toggle,
                    Action::Confirm,
                    Action::Cancel,
                ]
            }
            Screen::Main if matches!(self.popup, Some(Popup::Comment { .. })) => {
                &[Action::Confirm, Action::Cancel]
            }
            Screen::Main if self.popup.is_some() => {
                &[Action::Down, Action::Confirm, Action::Cancel]
            }
            Screen::Main => match self.current_item() {
                Some(Detail::PullRequest(pr)) if self.can_complete(&pr) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::Complete,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::PullRequest(_)) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::WorkItem(item)) if self.is_assigned_to_me(&item) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::ChangeColumn,
                    Action::Unassign,
                    Action::Tag,
                    Action::Comment,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::Build(_)) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::Confirm,
                    Action::OpenInBrowser,
                    Action::CopyLink,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::WorkItem(_)) => &[
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::ChangeColumn,
                    Action::AssignToMe,
                    Action::Comment,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                None => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::Sort,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
            },
        }
    }

    /// The item actions apply to: the shown detail when focused, else the selected row.
    fn current_item(&self) -> Option<Detail> {
        match self.focus.index() {
            Some(index) => self.data.detail(self.focus, self.selections[index]),
            None => self.detail.clone(),
        }
    }

    fn current_url(&self) -> Option<String> {
        let (item, config) = (self.current_item()?, self.config.as_ref()?);
        let url = match &item {
            Detail::PullRequest(pr) => api::pull_request_url(
                &config.organization,
                &config.project,
                &pr.repository.name,
                pr.pull_request_id,
            ),
            Detail::WorkItem(item) => {
                api::work_item_url(&config.organization, &config.project, item.id)
            }
            Detail::Build(build) => {
                api::build_results_url(&config.organization, &config.project, build.id)
            }
        };
        Some(url.to_string())
    }

    fn open_in_browser(&mut self) {
        let (Some(url), Some(config)) = (self.current_url(), &self.config) else {
            return;
        };
        let result = match &config.browser_command {
            Some(command) => {
                let mut parts = command.split_whitespace();
                match parts.next() {
                    Some(program) => std::process::Command::new(program)
                        .args(parts)
                        .arg(url.as_str())
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
            None => open::that_detached(url.as_str()),
        };
        if let Err(err) = result {
            tracing::warn!("failed to open browser: {err}");
            self.error = Some(format!("failed to open browser: {err}"));
        }
    }

    fn copy_number(&mut self) {
        let id = match self.current_item() {
            Some(Detail::PullRequest(pr)) => format!("!{}", pr.pull_request_id),
            Some(Detail::WorkItem(item)) => format!("#{}", item.id),
            Some(Detail::Build(_)) | None => return,
        };
        self.copy(id);
    }

    fn copy_link(&mut self) {
        if let Some(url) = self.current_url() {
            self.copy(url);
        }
    }

    fn copy(&mut self, text: String) {
        let copy = CopyToClipboard::to_clipboard_from(text.as_str());
        match crossterm::execute!(std::io::stdout(), copy) {
            Ok(()) => self.notice = Some((format!("copied {text}"), Instant::now())),
            Err(err) => self.error = Some(format!("failed to copy: {err}")),
        }
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < NOTICE_DURATION)
            .map(|(text, _)| text.as_str())
    }

    fn current_work_item(&self) -> Option<WorkItem> {
        match self.current_item()? {
            Detail::WorkItem(item) => Some(*item),
            Detail::PullRequest(_) | Detail::Build(_) => None,
        }
    }

    fn assign_to_me(&mut self) {
        let (Some(item), Some(client), Some(config), Some(user)) = (
            self.current_work_item(),
            self.client.clone(),
            self.config.clone(),
            self.user.as_ref(),
        ) else {
            return;
        };
        let (id, assignee) = (item.id, user.unique_name().to_string());
        self.spawn(async move {
            let result = client
                .assign_work_item(&config.organization, &config.project, id, &assignee)
                .await;
            Message::WorkItemUpdated { id, result }
        });
    }

    fn is_assigned_to_me(&self, item: &WorkItem) -> bool {
        let assignee = item.fields.assigned_to.as_ref().map(|a| a.id.as_str());
        self.user
            .as_ref()
            .is_some_and(|user| assignee == Some(user.id.as_str()))
    }

    /// Asks for the new column first; nothing changes until it is confirmed.
    fn unassign(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if self.is_assigned_to_me(&item) {
            self.change_column(true);
        } else {
            self.error = Some(format!("#{} is not assigned to you", item.id));
        }
    }

    fn tag(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if !self.is_assigned_to_me(&item) {
            self.error = Some(format!("#{} is not assigned to you", item.id));
            return;
        }
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            let result = client.tags(&config.organization, &config.project).await;
            Message::Tags { item, result }
        });
    }

    fn open_tag_picker(&mut self, item: &WorkItem, mut tags: Vec<String>) {
        let recent = self
            .config
            .as_ref()
            .map(|config| config.recent_tags.as_slice())
            .unwrap_or_default();
        tags.sort_by_key(|tag| {
            recent
                .iter()
                .position(|r| r.eq_ignore_ascii_case(tag))
                .unwrap_or(usize::MAX)
        });
        let current = item
            .fields
            .tags
            .as_deref()
            .unwrap_or_default()
            .split(';')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(String::from)
            .collect();
        self.popup = Some(Popup::Tags(TagPicker::new(item.id, current, tags)));
    }

    fn confirm_tags(&mut self, picker: TagPicker) {
        let Some(tag) = picker
            .selection
            .items
            .get(picker.selection.selected)
            .cloned()
        else {
            return;
        };
        let (Some(client), Some(config)) = (self.client.clone(), &mut self.config) else {
            return;
        };
        let (organization, project) = (config.organization.clone(), config.project.clone());
        let id = picker.work_item_id;
        let mut tags = picker.current.clone();
        if picker.has(&tag) {
            tags.retain(|current| !current.eq_ignore_ascii_case(&tag));
        } else {
            config.recent_tags.retain(|r| !r.eq_ignore_ascii_case(&tag));
            config.recent_tags.insert(0, tag.clone());
            config.recent_tags.truncate(MAX_RECENT_TAGS);
            let result = config.save();
            if let Err(err) = result {
                self.report_error(err);
            }
            tags.push(tag);
        }
        self.spawn(async move {
            let result = client
                .set_work_item_tags(&organization, &project, id, &tags)
                .await;
            Message::WorkItemUpdated { id, result }
        });
    }

    fn open_comment(&mut self) {
        if let Some(item) = self.current_work_item() {
            self.popup = Some(Popup::Comment {
                work_item_id: item.id,
                text: String::new(),
            });
        }
    }

    fn confirm_comment(&mut self, id: u32, text: String) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        // Markdown joins single newlines; trailing spaces make them line breaks.
        let text = text.replace('\n', "  \n");
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            let result = client
                .add_work_item_comment(&config.organization, &config.project, id, &text)
                .await;
            Message::WorkItemUpdated { id, result }
        });
    }

    fn change_column(&mut self, unassign: bool) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if let Some(board) = &self.board {
            let board = board.clone();
            self.open_column_picker(item, &board, unassign);
            return;
        }
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            let result = client
                .board(&config.organization, &config.project, &config.team)
                .await;
            Message::Board {
                item,
                unassign,
                result,
            }
        });
    }

    fn open_column_picker(&mut self, item: WorkItem, board: &Board, unassign: bool) {
        let Some(targets) = board.targets(&item.fields.work_item_type) else {
            self.error = Some(format!(
                "{} items are not on the board",
                item.fields.work_item_type
            ));
            return;
        };
        let labels = targets.iter().map(|t| t.label.clone()).collect();
        let mut selection = Selection::new(labels);
        let done = item.fields.board_column_done.unwrap_or(false);
        let current = item.fields.board_column.as_deref();
        selection.selected = targets
            .iter()
            .position(|t| Some(t.column.as_str()) == current && t.done == done)
            .or_else(|| {
                targets
                    .iter()
                    .position(|t| Some(t.column.as_str()) == current)
            })
            .unwrap_or(0);
        self.popup = Some(Popup::Column(ColumnPicker {
            work_item_id: item.id,
            targets,
            selection,
            unassign,
        }));
    }

    /// Column names of the team board, empty until it is loaded.
    pub fn board_columns(&self) -> Vec<String> {
        self.board
            .iter()
            .flat_map(|board| &board.columns)
            .map(|column| column.name.clone())
            .collect()
    }

    /// My PR with nothing blocking the merge.
    fn can_complete(&self, pr: &PullRequest) -> bool {
        self.data
            .pr_signals
            .get(&pr.pull_request_id)
            .is_some_and(|signals| signals.ready)
    }

    /// Asks for the merge strategy first; nothing is completed until it is confirmed.
    fn complete(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        if !self.can_complete(&pr) {
            self.error = Some(format!("!{} is not ready to complete", pr.pull_request_id));
            return;
        }
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            let result = client
                .merge_strategies(&config.organization, &config.project, &pr)
                .await;
            Message::MergeStrategies { pr, result }
        });
    }

    fn open_complete_picker(&mut self, pr: Box<PullRequest>, strategies: Vec<MergeStrategy>) {
        if strategies.is_empty() {
            self.error = Some(format!(
                "no merge strategy is allowed for !{}",
                pr.pull_request_id
            ));
            return;
        }
        let labels = strategies.iter().map(|s| s.label().into()).collect();
        let mut selection = Selection::new(labels);
        let last = self.config.as_ref().and_then(|c| c.merge_strategy);
        selection.selected = strategies
            .iter()
            .position(|s| Some(*s) == last)
            .unwrap_or(0);
        self.popup = Some(Popup::Complete(CompletePicker {
            pr,
            strategies,
            selection,
        }));
    }

    fn confirm_complete(&mut self, picker: CompletePicker) {
        let Some(strategy) = picker.strategies.get(picker.selection.selected).copied() else {
            return;
        };
        let (Some(client), Some(config)) = (self.client.clone(), &mut self.config) else {
            return;
        };
        config.merge_strategy = Some(strategy);
        let saved = config.save();
        let (config, pr) = (config.clone(), picker.pr);
        self.spawn(async move {
            let result = client
                .complete_pull_request(&config.organization, &config.project, &pr, strategy)
                .await;
            Message::PullRequestCompleted {
                id: pr.pull_request_id,
                result,
            }
        });
        if let Err(err) = saved {
            self.report_error(err);
        }
    }

    fn open_sort_picker(&mut self) {
        let (Some(kind), Some(config)) = (self.focus.sort_kind(), &self.config) else {
            return;
        };
        let (labels, selected): (Vec<String>, _) = match kind {
            SortKind::PullRequests => (
                PrSort::ALL.iter().map(|s| s.label().into()).collect(),
                PrSort::ALL
                    .iter()
                    .position(|s| *s == config.sort.pull_requests),
            ),
            SortKind::WorkItems => (
                WorkItemSort::ALL.iter().map(|s| s.label().into()).collect(),
                WorkItemSort::ALL
                    .iter()
                    .position(|s| *s == config.sort.work_items),
            ),
        };
        let mut selection = Selection::new(labels);
        selection.selected = selected.unwrap_or(0);
        self.popup = Some(Popup::Sort { kind, selection });
    }

    fn confirm_popup(&mut self) {
        match self.popup.take() {
            Some(Popup::Column(picker)) => self.confirm_column(picker),
            Some(Popup::Complete(picker)) => self.confirm_complete(picker),
            Some(Popup::Tags(picker)) => self.confirm_tags(picker),
            Some(Popup::Comment { work_item_id, text }) => self.confirm_comment(work_item_id, text),
            Some(Popup::Sort { kind, selection }) => self.confirm_sort(kind, selection.selected),
            Some(Popup::Types { selection, checked }) => self.confirm_types(selection, checked),
            Some(Popup::PrFilter { checked, .. }) => self.confirm_pr_filter(&checked),
            None => {}
        }
    }

    /// Board columns, used for the sprint overview and moving items.
    fn load_board(&self) {
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            Message::TeamBoard(
                client
                    .board(&config.organization, &config.project, &config.team)
                    .await,
            )
        });
    }

    fn load_work_item_types(&mut self) {
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        self.spawn(async move {
            Message::WorkItemTypes(
                client
                    .work_item_types(&config.organization, &config.project)
                    .await,
            )
        });
    }

    fn open_types_picker(&mut self, mut types: Vec<String>) {
        let Some(config) = &self.config else {
            return;
        };
        for kind in &config.work_item_types {
            if !types.contains(kind) {
                types.push(kind.clone());
            }
        }
        let checked = types
            .iter()
            .map(|kind| config.work_item_types.is_empty() || config.work_item_types.contains(kind))
            .collect();
        self.popup = Some(Popup::Types {
            selection: Selection::new(types),
            checked,
        });
    }

    fn confirm_types(&mut self, selection: Selection, checked: Vec<bool>) {
        let Some(config) = &mut self.config else {
            return;
        };
        config.work_item_types = checked_types(selection.items, &checked);
        if let Err(err) = config.save() {
            self.report_error(err);
            return;
        }
        self.data.my_work_items = None;
        self.data.ready_work_items = None;
        self.refresh();
    }

    fn open_pr_filter(&mut self) {
        let Some(config) = &self.config else {
            return;
        };
        let filter = &config.other_prs_filter;
        self.popup = Some(Popup::PrFilter {
            selection: Selection::new(vec!["Show approved".into(), "Show drafts".into()]),
            checked: vec![filter.show_approved, filter.show_drafts],
        });
    }

    fn confirm_pr_filter(&mut self, checked: &[bool]) {
        let Some(config) = &mut self.config else {
            return;
        };
        let filter = &mut config.other_prs_filter;
        filter.show_approved = checked[0];
        filter.show_drafts = checked[1];
        self.data.show_approved = filter.show_approved;
        self.data.show_drafts = filter.show_drafts;
        let result = config.save();
        self.clamp_selections();
        if let Err(err) = result {
            self.report_error(err);
        }
    }

    fn confirm_sort(&mut self, kind: SortKind, index: usize) {
        let Some(config) = &mut self.config else {
            return;
        };
        match kind {
            SortKind::PullRequests => {
                if let Some(sort) = PrSort::ALL.get(index) {
                    config.sort.pull_requests = *sort;
                }
            }
            SortKind::WorkItems => {
                if let Some(sort) = WorkItemSort::ALL.get(index) {
                    config.sort.work_items = *sort;
                }
            }
        }
        self.data.sort(&config.sort);
        let result = config.save();
        self.clamp_selections();
        if let Err(err) = result {
            self.report_error(err);
        }
    }

    fn confirm_column(&mut self, picker: ColumnPicker) {
        let (Some(board), Some(client), Some(config)) =
            (self.board.clone(), self.client.clone(), self.config.clone())
        else {
            return;
        };
        let Some(target) = picker.targets.get(picker.selection.selected).cloned() else {
            return;
        };
        let (id, unassign) = (picker.work_item_id, picker.unassign);
        self.spawn(async move {
            let (organization, project) = (&config.organization, &config.project);
            let result = if unassign {
                client
                    .unassign_work_item(organization, project, id, &board, &target)
                    .await
            } else {
                client
                    .move_work_item(organization, project, id, &board, &target)
                    .await
            };
            Message::WorkItemUpdated { id, result }
        });
    }

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(future.await);
        });
    }

    fn set_step(&mut self, step: SetupStep) {
        self.screen = Screen::Setup(step);
    }

    /// Drops a rejected PAT and asks for a new one. Returns true when handled.
    fn handle_rejected_pat(&mut self, err: &anyhow::Error) -> bool {
        let pat_rejected =
            api::is_unauthorized(err) && self.client.as_ref().is_some_and(|c| c.auth().uses_pat());
        if pat_rejected {
            self.client = None;
            tokio::spawn(Auth::forget_pat());
            self.set_step(SetupStep::EnterPat {
                input: String::new(),
                error: Some("The saved token was rejected. Enter a new one.".into()),
                cancelable: false,
            });
        }
        pat_rejected
    }

    fn fail(&mut self, err: anyhow::Error, retry: Retry) {
        tracing::error!("setup failed: {err:#}");
        if self.handle_rejected_pat(&err) {
            return;
        }
        self.set_step(SetupStep::Failed {
            error: format!("{err:#}"),
            retry,
        });
    }

    fn report_error(&mut self, err: anyhow::Error) {
        tracing::error!("{err:#}");
        if !self.handle_rejected_pat(&err) {
            self.error = Some(format!("{err:#}"));
        }
    }

    /// Refreshes on the configured interval while the main view is idle. 0 disables it.
    fn auto_refresh(&mut self) {
        let Some(interval) = self.config.as_ref().map(|c| c.refresh_interval_secs) else {
            return;
        };
        if interval > 0
            && matches!(self.screen, Screen::Main)
            && self.popup.is_none()
            && self.last_refresh.elapsed() >= Duration::from_secs(interval)
        {
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        let (Some(client), Some(config), Some(user)) =
            (self.client.clone(), self.config.clone(), self.user.clone())
        else {
            return;
        };
        self.error = None;
        self.last_refresh = Instant::now();
        if self.data.inbox_seen.is_none() && self.last_updated.is_some() {
            let seen = self.data.inbox().iter().map(|e| e.detail.key()).collect();
            self.data.inbox_seen = Some(seen);
        }
        self.data.show_approved = config.other_prs_filter.show_approved;
        self.data.show_drafts = config.other_prs_filter.show_drafts;

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::MyPullRequests(
                c.my_pull_requests(&cfg.organization, &cfg.project, &user_id)
                    .await,
            )
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::Builds(c.my_builds(&cfg.organization, &cfg.project, &user_id).await)
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::Mentions(
                c.mentioned_work_items(&cfg.organization, &cfg.project, &cfg.team, &user_id)
                    .await,
            )
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id);
        self.spawn(async move {
            Message::OtherPullRequests(
                c.other_pull_requests(
                    &cfg.organization,
                    &cfg.project,
                    &user_id,
                    &cfg.other_prs_filter,
                )
                .await,
            )
        });

        if let Some(detail) = &self.detail {
            self.load_detail(detail);
        }

        if let Some(ready_column) = config.ready_column.clone() {
            self.spawn(async move {
                Message::SprintWorkItems(
                    client
                        .sprint_work_items(
                            &config.organization,
                            &config.project,
                            &config.team,
                            &ready_column,
                            &config.work_item_types,
                        )
                        .await,
                )
            });
        }
    }

    fn retry(&mut self, retry: Retry) {
        match retry {
            Retry::Authenticate => {
                self.set_step(SetupStep::Loading("Signing in…"));
                self.authenticate();
            }
            Retry::Continue => self.continue_setup(),
            Retry::Projects(organization) => self.load_projects(organization),
            Retry::Teams(organization, project) => self.load_teams(organization, project),
            Retry::SaveConfig(config) => self.save_config(*config),
        }
    }

    fn can_enter_pat(&self) -> bool {
        self.config
            .as_ref()
            .is_none_or(|c| c.auth.method == AuthMethod::Auto)
    }

    fn authenticate(&mut self) {
        let auth = self
            .config
            .as_ref()
            .map(|c| c.auth.clone())
            .unwrap_or_default();
        self.spawn(async move { Message::Authenticated(Auth::resolve(auth).await) });
    }

    fn continue_setup(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match &self.config {
            None => {
                self.set_step(SetupStep::Loading("Loading organizations…"));
                self.spawn(async move { Message::Organizations(client.organizations().await) });
            }
            Some(config) if config.ready_column.is_none() => {
                let (organization, project, team) = (
                    config.organization.clone(),
                    config.project.clone(),
                    config.team.clone(),
                );
                self.set_step(SetupStep::Loading("Loading board columns…"));
                self.spawn(async move {
                    Message::BoardColumns(
                        client.board_columns(&organization, &project, &team).await,
                    )
                });
            }
            Some(config) => {
                let organization = config.organization.clone();
                self.set_step(SetupStep::Loading("Connecting…"));
                self.spawn(
                    async move { Message::Connected(client.current_user(&organization).await) },
                );
            }
        }
    }

    fn load_projects(&mut self, organization: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.set_step(SetupStep::Loading("Loading projects…"));
        self.spawn(async move {
            let result = client.projects(&organization).await;
            Message::Projects {
                organization,
                result,
            }
        });
    }

    fn load_teams(&mut self, organization: String, project: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.set_step(SetupStep::Loading("Loading teams…"));
        self.spawn(async move {
            let result = client.teams(&organization, &project).await;
            Message::Teams {
                organization,
                project,
                result,
            }
        });
    }

    fn save_config(&mut self, config: Config) {
        match config.save() {
            Ok(()) => {
                self.config = Some(config);
                self.continue_setup();
            }
            Err(err) => self.fail(err, Retry::SaveConfig(Box::new(config))),
        }
    }

    fn handle_message(&mut self, message: Message) {
        match message {
            Message::Authenticated(Ok(Some(auth))) => {
                self.client = Some(AdoClient::new(auth));
                self.continue_setup();
            }
            Message::Authenticated(Ok(None)) => self.set_step(SetupStep::EnterPat {
                input: String::new(),
                error: None,
                cancelable: false,
            }),
            Message::Authenticated(Err(err)) => self.fail(err, Retry::Authenticate),
            Message::Organizations(Ok(organizations)) if !organizations.is_empty() => {
                self.set_step(SetupStep::SelectOrganization(Selection::new(organizations)));
            }
            Message::Organizations(_) => {
                self.set_step(SetupStep::EnterOrganization(String::new()));
            }
            Message::Projects {
                organization,
                result,
            } => match result {
                Ok(projects) => self.set_step(SetupStep::SelectProject {
                    organization,
                    selection: Selection::new(projects),
                }),
                Err(err) => self.fail(err, Retry::Projects(organization)),
            },
            Message::Teams {
                organization,
                project,
                result,
            } => match result {
                Ok(teams) => self.set_step(SetupStep::SelectTeam {
                    organization,
                    project,
                    selection: Selection::new(teams),
                }),
                Err(err) => self.fail(err, Retry::Teams(organization, project)),
            },
            Message::BoardColumns(Ok(columns)) => {
                self.set_step(SetupStep::SelectReadyColumn(Selection::new(columns)));
            }
            Message::BoardColumns(Err(err)) => self.fail(err, Retry::Continue),
            Message::WorkItemTypes(Ok(types)) => self.open_types_picker(types),
            Message::WorkItemTypes(Err(err)) => self.report_error(err),
            Message::Connected(Ok(user)) => {
                self.user = Some(user);
                self.screen = Screen::Main;
                self.load_board();
                self.refresh();
            }
            Message::MyPullRequests(result) => match result {
                Ok(prs) => {
                    self.load_signals(&prs);
                    self.data
                        .pr_signals
                        .retain(|id, _| prs.iter().any(|pr| pr.pull_request_id == *id));
                    self.data.my_prs = Some(prs);
                    self.last_updated = Some(Instant::now());
                }
                Err(err) => self.report_error(err),
            },
            Message::OtherPullRequests(result) => match result {
                Ok(prs) => {
                    self.data.other_prs = Some(prs);
                    self.last_updated = Some(Instant::now());
                }
                Err(err) => self.report_error(err),
            },
            Message::SprintWorkItems(result) => match result {
                Ok(sprint) => {
                    self.data.sprint_name = Some(sprint.iteration_name);
                    self.data.sprint_start = sprint.start_date;
                    self.data.sprint_finish = sprint.finish_date;
                    self.data.my_work_items = Some(sprint.mine);
                    self.data.ready_work_items = Some(sprint.ready);
                    self.last_updated = Some(Instant::now());
                }
                Err(err) => self.report_error(err),
            },
            Message::Builds(result) => match result {
                Ok(builds) => self.data.builds = Some(builds),
                Err(err) => self.report_error(err),
            },
            Message::TeamBoard(result) => match result {
                Ok(board) => self.board = Some(board),
                Err(err) => tracing::warn!("failed to load board: {err:#}"),
            },
            Message::Mentions(result) => match result {
                Ok(mentions) => self.data.mentions = Some(mentions),
                Err(err) => self.report_error(err),
            },
            Message::MergeStrategies { pr, result } => match result {
                Ok(strategies) => self.open_complete_picker(pr, strategies),
                Err(err) => self.report_error(err),
            },
            Message::PullRequestCompleted { id, result } => match result {
                Ok(()) => {
                    tracing::info!(id, "pull request completed");
                    self.refresh();
                }
                Err(err) => self.report_error(err),
            },
            Message::Connected(Err(err)) => self.fail(err, Retry::Continue),
            Message::PullRequestDetails { id, result } => {
                if self.detail.as_ref().is_some_and(|d| d.is_pull_request(id)) {
                    match result {
                        Ok(info) => {
                            self.detail_info = Some(DetailInfo::PullRequest(info));
                            self.load_images();
                        }
                        Err(err) => self.report_error(err),
                    }
                }
            }
            Message::Board {
                item,
                unassign,
                result,
            } => match result {
                Ok(board) => {
                    self.open_column_picker(item, &board, unassign);
                    self.board = Some(board);
                }
                Err(err) => self.report_error(err),
            },
            Message::PullRequestSignals { id, result } => {
                let pr = self
                    .data
                    .my_prs
                    .iter()
                    .flatten()
                    .find(|pr| pr.pull_request_id == id);
                match (pr, result) {
                    (Some(pr), Ok(details)) => {
                        let signals = Signals::new(pr, &details);
                        self.data.pr_signals.insert(id, signals);
                    }
                    (_, Err(err)) => tracing::warn!(id, "failed to load PR signals: {err:#}"),
                    (None, _) => {}
                }
            }
            Message::Image { url, result } => self.images.insert(url, result),
            Message::WorkItemUpdated { id, result } => match result {
                Ok(()) => {
                    tracing::info!(id, "work item updated");
                    self.refresh();
                }
                Err(err) => self.report_error(err),
            },
            Message::Tags { item, result } => match result {
                Ok(tags) => self.open_tag_picker(&item, tags),
                Err(err) => self.report_error(err),
            },
            Message::WorkItemDetails { id, result } => {
                if self.detail.as_ref().is_some_and(|d| d.is_work_item(id)) {
                    match result {
                        Ok(info) => {
                            self.detail_info = Some(DetailInfo::WorkItem(info));
                            self.load_images();
                        }
                        Err(err) => self.report_error(err),
                    }
                }
            }
        }
        if let Some(config) = &self.config {
            self.data.sort(&config.sort);
        }
        self.clamp_selections();
        self.sync_detail();
    }

    fn switch_tab(&mut self, next: bool) {
        let Some(detail) = &self.detail else {
            return;
        };
        let tabs = detail.tabs();
        let index = tabs
            .iter()
            .position(|tab| *tab == self.detail_tab)
            .unwrap_or(0);
        let index = if next {
            (index + 1) % tabs.len()
        } else {
            (index + tabs.len() - 1) % tabs.len()
        };
        self.detail_tab = tabs[index];
        self.detail_scroll = 0;
    }

    fn clamp_selections(&mut self) {
        for (index, panel) in LEFT_PANELS.iter().enumerate() {
            let len = self.data.len(*panel);
            self.selections[index] = self.selections[index].min(len.saturating_sub(1));
        }
    }

    fn move_selection(&mut self, down: bool) {
        let Some(index) = self.focus.index() else {
            return;
        };
        let len = self.data.len(self.focus);
        let selected = &mut self.selections[index];
        *selected = if down {
            (*selected + 1).min(len.saturating_sub(1))
        } else {
            selected.saturating_sub(1)
        };
    }

    fn move_focus(&mut self, action: Action) {
        match (action, self.focus.index()) {
            (Action::FocusLeft, None) => self.focus = self.last_left,
            (Action::FocusRight, Some(_)) => {
                self.last_left = self.focus;
                self.focus = Panel::Detail;
            }
            (Action::FocusUp, Some(index)) if index > 0 => self.focus = LEFT_PANELS[index - 1],
            (Action::FocusDown, Some(index)) if index + 1 < LEFT_PANELS.len() => {
                self.focus = LEFT_PANELS[index + 1];
            }
            _ => {}
        }
    }

    fn scroll_detail(&mut self, down: bool) {
        self.detail_scroll = if down {
            self.detail_scroll.saturating_add(1)
        } else {
            self.detail_scroll.saturating_sub(1)
        };
    }

    fn open_selected(&mut self) {
        let Some(index) = self.focus.index() else {
            return;
        };
        let Some(detail) = self.data.detail(self.focus, self.selections[index]) else {
            return;
        };
        self.load_detail(&detail);
        if let Some(seen) = &mut self.data.inbox_seen {
            seen.insert(detail.key());
        }
        self.detail = Some(detail);
        self.load_images();
        self.detail_info = None;
        self.detail_scroll = 0;
        self.detail_tab = DetailTab::Overview;
    }

    /// Starts downloading images in the shown detail that are not cached yet.
    fn load_images(&self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut texts: Vec<&str> = Vec::new();
        let mut rich: Vec<&RichText> = Vec::new();
        if let Some(Detail::PullRequest(pr)) = &self.detail {
            texts.extend(pr.description.as_deref());
        }
        match &self.detail_info {
            Some(DetailInfo::PullRequest(info)) => texts.extend(
                info.threads
                    .iter()
                    .flat_map(|thread| &thread.comments)
                    .filter_map(|comment| comment.content.as_deref()),
            ),
            Some(DetailInfo::WorkItem(info)) => {
                rich.extend(
                    [
                        &info.description,
                        &info.repro_steps,
                        &info.acceptance_criteria,
                    ]
                    .into_iter()
                    .flatten(),
                );
                rich.extend(info.comments.iter().map(|comment| &comment.text));
            }
            None => {}
        }
        let urls = texts
            .into_iter()
            .flat_map(image_urls)
            .chain(rich.into_iter().flat_map(RichText::image_urls))
            .collect();
        for url in self.images.start_loading(urls) {
            let client = client.clone();
            self.spawn(async move {
                let result = match client.download(&url).await {
                    Ok(bytes) => tokio::task::spawn_blocking(move || {
                        image::load_from_memory(&bytes).map_err(anyhow::Error::from)
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result),
                    Err(err) => Err(err),
                };
                Message::Image { url, result }
            });
        }
    }

    fn load_signals(&self, prs: &[PullRequest]) {
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        for pr in prs {
            let (client, config, pr) = (client.clone(), config.clone(), pr.clone());
            self.spawn(async move {
                let result = client
                    .pull_request_details(&config.organization, &config.project, &pr)
                    .await;
                Message::PullRequestSignals {
                    id: pr.pull_request_id,
                    result,
                }
            });
        }
    }

    fn load_detail(&self, detail: &Detail) {
        let (Some(client), Some(config)) = (self.client.clone(), self.config.clone()) else {
            return;
        };
        match detail {
            Detail::PullRequest(pr) => {
                let (id, pr) = (pr.pull_request_id, pr.clone());
                self.spawn(async move {
                    let result = client
                        .pull_request_details(&config.organization, &config.project, &pr)
                        .await;
                    Message::PullRequestDetails { id, result }
                });
            }
            Detail::WorkItem(item) => {
                let id = item.id;
                self.spawn(async move {
                    let result = client
                        .work_item_details(&config.organization, &config.project, id)
                        .await;
                    Message::WorkItemDetails { id, result }
                });
            }
            Detail::Build(_) => {}
        }
    }

    /// Back to the sprint overview.
    fn close_detail(&mut self) {
        self.detail = None;
        self.detail_info = None;
        if self.focus == Panel::Detail {
            self.focus = self.last_left;
        }
    }

    fn sync_detail(&mut self) {
        if let Some(fresh) = self.detail.as_ref().and_then(|d| self.data.find(d)) {
            self.detail = Some(fresh);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if let Screen::Setup(step) = &mut self.screen
            && let Some(input) = step.input_mut()
        {
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    return;
                }
                KeyCode::Backspace => {
                    input.pop();
                    return;
                }
                _ => {}
            }
        }
        if let Some(Popup::Tags(picker)) = &mut self.popup {
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    picker.query.push(c);
                    picker.filter();
                    return;
                }
                KeyCode::Backspace => {
                    picker.query.pop();
                    picker.filter();
                    return;
                }
                _ => {}
            }
        }
        if let Some(Popup::Comment { text, .. }) = &mut self.popup {
            match key.code {
                KeyCode::Enter
                    if key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    text.push('\n');
                    return;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text.push(c);
                    return;
                }
                KeyCode::Backspace => {
                    text.pop();
                    return;
                }
                _ => {}
            }
        }
        if let Some(action) = Action::from_key(key) {
            self.apply(action);
        }
    }

    fn apply(&mut self, action: Action) {
        if self.show_help {
            match action {
                Action::Cancel | Action::Help => self.show_help = false,
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }
        if matches!(self.popup, Some(Popup::Comment { .. })) {
            match action {
                Action::Confirm => self.confirm_popup(),
                Action::Cancel => self.popup = None,
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }
        if let Some(popup) = &mut self.popup {
            let selection = match popup {
                Popup::Column(picker) => &mut picker.selection,
                Popup::Complete(picker) => &mut picker.selection,
                Popup::Tags(picker) => &mut picker.selection,
                Popup::Comment { .. } => return,
                Popup::Sort { selection, .. }
                | Popup::Types { selection, .. }
                | Popup::PrFilter { selection, .. } => selection,
            };
            match action {
                Action::Toggle => {
                    if let Popup::Types { selection, checked }
                    | Popup::PrFilter { selection, checked } = popup
                        && let Some(checked) = checked.get_mut(selection.selected)
                    {
                        *checked = !*checked;
                    }
                }
                Action::Up => selection.previous(),
                Action::Down => selection.next(),
                Action::Confirm => self.confirm_popup(),
                Action::Cancel => self.popup = None,
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }
        match action {
            Action::Quit => self.should_quit = true,
            Action::Cancel => match self.screen {
                Screen::Setup(SetupStep::EnterPat {
                    cancelable: true, ..
                }) => self.screen = Screen::Main,
                Screen::Setup(_) => self.should_quit = true,
                Screen::Main => self.close_detail(),
            },
            Action::Help => self.show_help = matches!(self.screen, Screen::Main),
            Action::ChangeToken => match self.screen {
                Screen::Main => self.set_step(SetupStep::EnterPat {
                    input: String::new(),
                    error: None,
                    cancelable: true,
                }),
                Screen::Setup(SetupStep::Failed {
                    retry: Retry::Authenticate,
                    ..
                }) if self.can_enter_pat() => self.set_step(SetupStep::EnterPat {
                    input: String::new(),
                    error: None,
                    cancelable: false,
                }),
                Screen::Setup(_) => {}
            },
            Action::Up | Action::Down => match &mut self.screen {
                Screen::Main if self.focus == Panel::Detail => {
                    self.scroll_detail(action == Action::Down);
                }
                Screen::Main => self.move_selection(action == Action::Down),
                Screen::Setup(step) => {
                    if let Some(selection) = step.selection_mut() {
                        if action == Action::Up {
                            selection.previous();
                        } else {
                            selection.next();
                        }
                    }
                }
            },
            Action::Confirm => match self.screen {
                Screen::Main => self.open_selected(),
                Screen::Setup(_) => self.confirm_setup_step(),
            },
            Action::OpenInBrowser
            | Action::Copy
            | Action::CopyLink
            | Action::AssignToMe
            | Action::Unassign
            | Action::Tag
            | Action::Comment
            | Action::ChangeColumn
            | Action::Complete
            | Action::Sort
            | Action::Filter
            | Action::Toggle
            | Action::NextTab
            | Action::PrevTab
                if !matches!(self.screen, Screen::Main) => {}
            Action::OpenInBrowser => self.open_in_browser(),
            Action::Copy => self.copy_number(),
            Action::CopyLink => self.copy_link(),
            Action::Sort => self.open_sort_picker(),
            Action::Filter => match self.current_item() {
                Some(Detail::PullRequest(_)) => self.open_pr_filter(),
                _ => self.load_work_item_types(),
            },
            Action::Toggle => {}
            Action::NextTab | Action::PrevTab => self.switch_tab(action == Action::NextTab),
            Action::AssignToMe => self.assign_to_me(),
            Action::ChangeColumn => self.change_column(false),
            Action::Complete => self.complete(),
            Action::Unassign => self.unassign(),
            Action::Tag => self.tag(),
            Action::Comment => self.open_comment(),
            Action::FocusLeft | Action::FocusRight | Action::FocusUp | Action::FocusDown => {
                if matches!(self.screen, Screen::Main) {
                    self.move_focus(action);
                }
            }
            Action::Reload => match &self.screen {
                Screen::Main => self.refresh(),
                Screen::Setup(SetupStep::Failed { .. }) => {
                    if let Screen::Setup(SetupStep::Failed { retry, .. }) =
                        std::mem::replace(&mut self.screen, Screen::Main)
                    {
                        self.retry(retry);
                    }
                }
                Screen::Setup(_) => {}
            },
        }
    }

    fn confirm_setup_step(&mut self) {
        let Screen::Setup(step) = std::mem::replace(&mut self.screen, Screen::Main) else {
            return;
        };
        match step {
            SetupStep::EnterPat { input, .. } if !input.trim().is_empty() => {
                let pat = input.trim().to_string();
                self.set_step(SetupStep::Loading("Saving token…"));
                self.spawn(async move {
                    Message::Authenticated(Auth::with_new_pat(pat).await.map(Some))
                });
            }
            SetupStep::EnterOrganization(input) if !organization_name(&input).is_empty() => {
                self.load_projects(organization_name(&input).to_string());
            }
            SetupStep::SelectOrganization(selection) => match selection.current() {
                Some(organization) => self.load_projects(organization.to_string()),
                None => self.set_step(SetupStep::SelectOrganization(selection)),
            },
            SetupStep::SelectProject {
                organization,
                selection,
            } => match selection.current() {
                Some(project) => self.load_teams(organization, project.to_string()),
                None => self.set_step(SetupStep::SelectProject {
                    organization,
                    selection,
                }),
            },
            SetupStep::SelectTeam {
                organization,
                project,
                selection,
            } => match selection.current() {
                Some(team) => {
                    let config = Config::new(organization, project, team.to_string());
                    self.theme = config.colors.clone();
                    self.save_config(config);
                }
                None => self.set_step(SetupStep::SelectTeam {
                    organization,
                    project,
                    selection,
                }),
            },
            SetupStep::SelectReadyColumn(selection) => {
                match (selection.current(), self.config.clone()) {
                    (Some(column), Some(mut config)) => {
                        config.ready_column = Some(column.to_string());
                        self.save_config(config);
                    }
                    _ => self.set_step(SetupStep::SelectReadyColumn(selection)),
                }
            }
            step => self.set_step(step),
        }
    }
}

/// The ticked types; empty (meaning all) when every type is ticked.
fn checked_types(items: Vec<String>, checked: &[bool]) -> Vec<String> {
    if checked.iter().all(|checked| *checked) {
        return Vec::new();
    }
    items
        .into_iter()
        .zip(checked)
        .filter_map(|(kind, checked)| checked.then_some(kind))
        .collect()
}

/// Cells over areas where images were drawn, rewritten so the terminal drops their old pixels.
fn stale_cells(buffer: &Buffer, areas: &[Rect]) -> Vec<(u16, u16, Cell)> {
    areas
        .iter()
        .map(|area| area.intersection(buffer.area))
        .flat_map(|area| area.positions())
        .map(|position| (position.x, position.y, buffer[position].clone()))
        .filter(|(_, _, cell)| cell.diff_option != CellDiffOption::Skip)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::PullRequestDetails;
    use crate::test_support::{config, ids_of_prs, pull_request, work_item};

    #[test]
    fn stale_cells_skip_cells_covered_by_images() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 2));
        buffer[(1, 0)].set_symbol("x");
        buffer[(2, 0)].set_diff_option(CellDiffOption::Skip);

        let cells = stale_cells(&buffer, &[Rect::new(1, 0, 5, 1)]);

        let positions: Vec<_> = cells.iter().map(|(x, y, _)| (*x, *y)).collect();
        assert_eq!(positions, [(1, 0)]);
        assert_eq!(cells[0].2.symbol(), "x");
    }

    fn app() -> App {
        let mut app = App::new(Some(config()), None);
        app.screen = Screen::Main;
        app.focus = Panel::MyPrs;
        app.data = Data {
            my_prs: Some(vec![
                pull_request(1, "First", "me", "2026-10-03T10:00:00Z"),
                pull_request(2, "Second", "me", "2026-10-02T10:00:00Z"),
                pull_request(3, "Third", "me", "2026-10-01T10:00:00Z"),
            ]),
            other_prs: Some(vec![pull_request(4, "Theirs", "u", "2026-10-01T10:00:00Z")]),
            my_work_items: Some(vec![
                work_item(10, "Story", "User Story", "Active"),
                work_item(11, "Bug", "Bug", "New"),
            ]),
            ready_work_items: Some(vec![work_item(12, "Ready", "User Story", "Ready")]),
            sprint_name: Some("Sprint 1".into()),
            ..Default::default()
        };
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn open_pull_request(app: &mut App) {
        press(app, KeyCode::Enter);
        assert!(app.detail.as_ref().is_some_and(|d| d.is_pull_request(1)));
    }

    #[test]
    fn focus_moves_between_boxes_and_stops_at_edges() {
        let mut app = app();

        ctrl(&mut app, 'k');
        assert_eq!(app.focus, Panel::Inbox);
        ctrl(&mut app, 'k');
        assert_eq!(app.focus, Panel::Inbox);
        for _ in 0..5 {
            ctrl(&mut app, 'j');
        }
        assert_eq!(app.focus, Panel::Builds);
    }

    #[test]
    fn focus_returns_to_last_left_box() {
        let mut app = app();
        ctrl(&mut app, 'j');
        ctrl(&mut app, 'j');

        ctrl(&mut app, 'l');
        assert_eq!(app.focus, Panel::Detail);
        ctrl(&mut app, 'h');

        assert_eq!(app.focus, Panel::OtherPrs);
    }

    #[test]
    fn selection_stays_within_list() {
        let mut app = app();

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selections[1], 0);
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.selections[1], 2);
    }

    #[test]
    fn selection_is_clamped_when_list_shrinks() {
        let mut app = app();
        app.selections[1] = 2;

        app.handle_message(Message::MyPullRequests(Ok(vec![pull_request(
            1,
            "Only",
            "me",
            "2026-10-01T10:00:00Z",
        )])));

        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn loaded_lists_use_configured_sort() {
        let mut app = app();

        app.handle_message(Message::MyPullRequests(Ok(vec![
            pull_request(1, "Old", "me", "2026-10-01T10:00:00Z"),
            pull_request(2, "New", "me", "2026-10-05T10:00:00Z"),
        ])));

        assert_eq!(ids_of_prs(app.data.my_prs.as_ref().unwrap()), [2, 1]);
        assert!(app.last_updated.is_some());
    }

    #[test]
    fn enter_opens_selected_item_on_overview() {
        let mut app = app();
        app.detail_tab = DetailTab::Checks;
        app.detail_scroll = 5;
        press(&mut app, KeyCode::Char('j'));

        press(&mut app, KeyCode::Enter);

        assert!(app.detail.as_ref().is_some_and(|d| d.is_pull_request(2)));
        assert_eq!(app.detail_tab, DetailTab::Overview);
        assert_eq!(app.detail_scroll, 0);
        assert!(app.detail_info.is_none());
    }

    #[test]
    fn j_scrolls_detail_when_it_has_focus() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));

        assert_eq!(app.detail_scroll, 1);
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn tab_cycles_through_detail_tabs() {
        let mut app = app();
        open_pull_request(&mut app);
        app.detail_scroll = 3;

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Comments);
        assert_eq!(app.detail_scroll, 0);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Checks);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Overview);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.detail_tab, DetailTab::Checks);
    }

    #[test]
    fn tab_without_detail_does_nothing() {
        let mut app = app();

        press(&mut app, KeyCode::Tab);

        assert_eq!(app.detail_tab, DetailTab::Overview);
    }

    #[test]
    fn shown_detail_is_updated_after_refresh() {
        let mut app = app();
        open_pull_request(&mut app);

        app.handle_message(Message::MyPullRequests(Ok(vec![pull_request(
            1,
            "Renamed",
            "me",
            "2026-10-01T10:00:00Z",
        )])));

        assert!(matches!(&app.detail, Some(Detail::PullRequest(pr)) if pr.title == "Renamed"));
    }

    #[test]
    fn details_for_another_item_are_ignored() {
        let mut app = app();
        open_pull_request(&mut app);
        let details = || PullRequestDetails {
            threads: Vec::new(),
            statuses: Vec::new(),
            policies: Vec::new(),
            work_items: Vec::new(),
        };

        app.handle_message(Message::PullRequestDetails {
            id: 99,
            result: Ok(details()),
        });
        assert!(app.detail_info.is_none());
        app.handle_message(Message::PullRequestDetails {
            id: 1,
            result: Ok(details()),
        });
        assert!(matches!(app.detail_info, Some(DetailInfo::PullRequest(_))));
    }

    #[test]
    fn help_popup_blocks_other_actions() {
        let mut app = app();

        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selections[1], 0);
        press(&mut app, KeyCode::Esc);

        assert!(!app.show_help);
        assert!(!app.should_quit);
    }

    #[test]
    fn sort_picker_preselects_current_sort() {
        let mut app = app();
        app.config.as_mut().unwrap().sort.work_items = WorkItemSort::Changed;
        ctrl(&mut app, 'j');

        press(&mut app, KeyCode::Char('S'));

        let Some(Popup::Sort { kind, selection }) = &app.popup else {
            panic!("sort picker not open");
        };
        assert_eq!(*kind, SortKind::WorkItems);
        assert_eq!(selection.current(), Some("changed"));
        press(&mut app, KeyCode::Esc);
        assert!(app.popup.is_none());
    }

    #[test]
    fn sort_picker_needs_a_list_focus() {
        let mut app = app();
        ctrl(&mut app, 'l');

        press(&mut app, KeyCode::Char('S'));

        assert!(app.popup.is_none());
    }

    #[test]
    fn popup_selection_moves_with_j_and_k() {
        let mut app = app();
        press(&mut app, KeyCode::Char('S'));

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));

        let Some(Popup::Sort { selection, .. }) = &app.popup else {
            panic!("sort picker not open");
        };
        assert_eq!(selection.current(), Some("oldest"));
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn types_picker_ticks_configured_types() {
        let mut app = app();
        app.config.as_mut().unwrap().work_item_types = vec!["Bug".into(), "Retired".into()];

        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        let Some(Popup::Types { selection, checked }) = &app.popup else {
            panic!("types picker not open");
        };
        assert_eq!(selection.items, ["Bug", "Task", "Retired"]);
        assert_eq!(checked, &[true, false, true]);
    }

    #[test]
    fn types_picker_ticks_all_without_filter() {
        let mut app = app();

        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        assert!(
            matches!(&app.popup, Some(Popup::Types { checked, .. }) if checked == &[true, true])
        );
    }

    #[test]
    fn space_toggles_type() {
        let mut app = app();
        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char(' '));

        assert!(
            matches!(&app.popup, Some(Popup::Types { checked, .. }) if checked == &[true, false])
        );
    }

    #[test]
    fn checked_types_keep_ticked_types() {
        let types = checked_types(vec!["Bug".into(), "Task".into()], &[false, true]);

        assert_eq!(types, ["Task"]);
    }

    #[test]
    fn all_or_no_ticked_types_mean_all() {
        assert!(checked_types(vec!["Bug".into(), "Task".into()], &[true, true]).is_empty());
        assert!(checked_types(vec!["Bug".into()], &[false]).is_empty());
    }

    #[test]
    fn esc_in_setup_quits() {
        let mut app = App::new(None, None);

        press(&mut app, KeyCode::Esc);

        assert!(app.should_quit);
    }

    #[test]
    fn esc_in_cancelable_pat_dialog_returns_to_main() {
        let mut app = app();

        press(&mut app, KeyCode::Char('T'));
        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::EnterPat { .. })
        ));
        press(&mut app, KeyCode::Esc);

        assert!(matches!(app.screen, Screen::Main));
        assert!(!app.should_quit);
    }

    #[test]
    fn text_input_takes_letters_instead_of_actions() {
        let mut app = app();
        press(&mut app, KeyCode::Char('T'));

        for c in "qj?".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Backspace);

        assert!(!app.should_quit);
        assert!(
            matches!(&app.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input == "qj")
        );
    }

    #[test]
    fn q_quits_from_main() {
        let mut app = app();

        press(&mut app, KeyCode::Char('q'));

        assert!(app.should_quit);
    }

    #[test]
    fn toolbar_actions_follow_context() {
        let mut app = app();
        assert!(app.actions().contains(&Action::OpenInBrowser));
        assert!(!app.actions().contains(&Action::ChangeColumn));

        ctrl(&mut app, 'j');
        assert!(app.actions().contains(&Action::ChangeColumn));

        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.actions(), [Action::Cancel]);
    }

    fn me() -> CurrentUser {
        serde_json::from_value(serde_json::json!({ "id": "me-id", "providerDisplayName": "Me" }))
            .unwrap()
    }

    fn board() -> Board {
        Board {
            column_field: "Column".into(),
            done_field: "Column.Done".into(),
            columns: serde_json::from_value(serde_json::json!([
                { "name": "Ready", "stateMappings": { "User Story": "New", "Bug": "New" } },
                { "name": "Active", "isSplit": true, "stateMappings": { "User Story": "Active", "Bug": "Active" } }
            ]))
            .unwrap(),
        }
    }

    fn assign_first_item_to(app: &mut App, id: &str) {
        let item = &mut app.data.my_work_items.as_mut().unwrap()[0];
        item.fields.assigned_to = Some(
            serde_json::from_value(serde_json::json!({ "id": id, "displayName": "Someone" }))
                .unwrap(),
        );
    }

    #[test]
    fn unassign_asks_for_new_status_first() {
        let mut app = app();
        app.user = Some(me());
        app.board = Some(board());
        assign_first_item_to(&mut app, "me-id");
        ctrl(&mut app, 'j');

        press(&mut app, KeyCode::Char('u'));

        let Some(Popup::Column(picker)) = &app.popup else {
            panic!("column picker not open");
        };
        assert!(picker.unassign);
        assert_eq!(picker.work_item_id, 10);
        assert_eq!(picker.selection.current(), Some("Active (Doing)"));
    }

    #[test]
    fn esc_cancels_unassign() {
        let mut app = app();
        app.user = Some(me());
        app.board = Some(board());
        assign_first_item_to(&mut app, "me-id");
        ctrl(&mut app, 'j');
        press(&mut app, KeyCode::Char('u'));

        press(&mut app, KeyCode::Esc);

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }

    #[test]
    fn unassign_needs_item_assigned_to_me() {
        let mut app = app();
        app.user = Some(me());
        app.board = Some(board());
        assign_first_item_to(&mut app, "someone-else");
        ctrl(&mut app, 'j');

        press(&mut app, KeyCode::Char('u'));

        assert!(app.popup.is_none());
        assert_eq!(app.error.as_deref(), Some("#10 is not assigned to you"));
    }

    #[test]
    fn change_status_does_not_unassign() {
        let mut app = app();
        app.board = Some(board());
        ctrl(&mut app, 'j');

        press(&mut app, KeyCode::Char('s'));

        assert!(matches!(&app.popup, Some(Popup::Column(picker)) if !picker.unassign));
    }

    #[test]
    fn toolbar_offers_unassign_only_for_my_items() {
        let mut app = app();
        app.user = Some(me());
        assign_first_item_to(&mut app, "me-id");
        ctrl(&mut app, 'j');
        assert!(app.actions().contains(&Action::Unassign));

        press(&mut app, KeyCode::Char('j'));

        assert!(!app.actions().contains(&Action::Unassign));
        assert!(app.actions().contains(&Action::AssignToMe));
    }

    #[test]
    fn signals_are_stored_for_my_pull_requests() {
        let mut app = app();
        let details = PullRequestDetails {
            threads: serde_json::from_value(serde_json::json!([
                { "status": "active", "comments": [] }
            ]))
            .unwrap(),
            statuses: Vec::new(),
            policies: Vec::new(),
            work_items: Vec::new(),
        };

        app.handle_message(Message::PullRequestSignals {
            id: 2,
            result: Ok(details),
        });

        assert_eq!(app.data.pr_signals[&2].unresolved, 1);
    }

    #[test]
    fn signals_for_removed_pull_requests_are_dropped() {
        let mut app = app();
        app.data.pr_signals.insert(3, Signals::default());

        app.handle_message(Message::MyPullRequests(Ok(vec![pull_request(
            1,
            "Only",
            "me",
            "2026-10-01T10:00:00Z",
        )])));

        assert!(!app.data.pr_signals.contains_key(&3));
    }
}
