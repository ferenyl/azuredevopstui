mod action;
mod actions;
mod message;
mod messages;
mod popups;
mod refresh;
mod setup;
mod state;
mod toolbar;
mod wizard;

pub use action::{Action, HELP};
pub use setup::{Retry, Selection, SetupStep};
pub use state::{
    ColumnPicker, CompletePicker, Data, Detail, DetailInfo, DetailTab, InboxEntry, LEFT_PANELS,
    Panel, Popup, Signals, SortKind, SprintPicker, TagPicker,
};

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::Rect;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::api::{AdoClient, Board, CurrentUser, WorkItem};
use crate::config::Config;
use crate::images::Images;
use crate::notify;
use crate::theme::Theme;
use message::Message;

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
    /// Iteration path of the shown sprint; `None` is the team's current sprint.
    sprint: Option<String>,
    pub show_help: bool,
    pub last_updated: Option<Instant>,
    last_refresh: Instant,
    board: Option<Board>,
    last_left: Panel,
    pub error: Option<String>,
    /// Short confirmation shown in the toolbar until it expires.
    notice: Option<(String, Instant)>,
    /// Status of inbox entries last notified about, by key; `None` until the first refresh after loading.
    notified: Option<HashMap<String, String>>,
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
            sprint: config.as_ref().and_then(|c| c.default_sprint.clone()),
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
            notified: None,
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
                    Some(Ok(Event::Paste(text))) => self.handle_paste(&text),
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(err.into()),
                    None => break,
                },
                Some(message) = self.rx.recv() => {
                    self.handle_message(message);
                    self.notify_new_inbox();
                }
                _ = ticker.tick() => self.auto_refresh(),
            }
        }
        Ok(())
    }

    /// The item actions apply to: the shown detail when focused, else the selected row.
    fn current_item(&self) -> Option<Detail> {
        match self.focus.index() {
            Some(index) => self.data.detail(self.focus, self.selections[index]),
            None => self.detail.clone(),
        }
    }

    fn connection(&self) -> Option<(AdoClient, Config)> {
        Some((self.client.clone()?, self.config.clone()?))
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

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(future.await);
        });
    }

    /// Inbox entries that showed up or changed status after the first load and are not notified yet.
    /// Marks them as notified. Empty when notifications are off.
    fn new_inbox_entries(&mut self) -> Vec<Detail> {
        if !self.config.as_ref().is_some_and(|c| c.notifications) {
            return Vec::new();
        }
        let Some(notified) = &mut self.notified else {
            return Vec::new();
        };
        self.data
            .inbox()
            .into_iter()
            .map(|entry| entry.detail)
            .filter(|detail| {
                let status = self.data.status(detail);
                notified.insert(detail.key(), status.clone()).as_ref() != Some(&status)
            })
            .collect()
    }

    /// Sends a desktop notification for each new inbox entry.
    fn notify_new_inbox(&mut self) {
        let entries = self.new_inbox_entries();
        let (summary, body, url) = match entries.as_slice() {
            [] => return,
            [detail] => {
                let (summary, title) = match detail {
                    Detail::PullRequest(pr) => (
                        format!("Pull request !{}", pr.pull_request_id),
                        pr.title.clone(),
                    ),
                    Detail::WorkItem(item) => (
                        format!("Mentioned in #{}", item.id),
                        item.fields.title.clone(),
                    ),
                    Detail::Build(build) => (
                        format!("Build {} failed", build.build_number),
                        build.definition.name.clone(),
                    ),
                };
                let status = self.data.status(detail);
                let body = if status.is_empty() {
                    title
                } else {
                    format!("{title}\n{status}")
                };
                (summary, body, self.detail_url(detail))
            }
            details => (
                format!("{} new items in your inbox", details.len()),
                details
                    .iter()
                    .map(|detail| format!("{} {}", detail.key(), detail.title()))
                    .collect::<Vec<_>>()
                    .join("\n"),
                None,
            ),
        };
        self.spawn(async move { Message::Notified(notify::send(summary, body, url).await) });
    }

    /// After the first load, remembers the current inbox as seen and notified.
    fn track_inbox(&mut self) {
        if self.data.inbox_seen.is_none() && self.last_updated.is_some() {
            let inbox = self.data.inbox();
            let seen: HashSet<String> = inbox.iter().map(|e| e.detail.key()).collect();
            self.notified = Some(
                inbox
                    .iter()
                    .map(|e| (e.detail.key(), self.data.status(&e.detail)))
                    .collect(),
            );
            self.data.inbox_seen = Some(seen);
        }
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

    pub(crate) fn handle_paste(&mut self, pasted: &str) {
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(Popup::Comment { text, .. }) = &mut self.popup {
            text.push_str(&pasted);
            return;
        }
        let line = pasted.lines().next().unwrap_or_default();
        match &mut self.popup {
            Some(Popup::BuildFilter { text }) => text.push_str(line),
            Some(Popup::Tags(picker)) => {
                picker.query.push_str(line);
                picker.filter();
            }
            Some(Popup::Sprint(picker)) => {
                picker.query.push_str(line);
                picker.filter();
            }
            _ => {
                if let Screen::Setup(step) = &mut self.screen
                    && let Some(input) = step.input_mut()
                {
                    input.push_str(line);
                }
            }
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
        if self.handle_popup_key(key) {
            return;
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
        if self.apply_popup(action) {
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
            | Action::Sprint
            | Action::Toggle
            | Action::NextTab
            | Action::PrevTab
                if !matches!(self.screen, Screen::Main) => {}
            Action::OpenInBrowser => self.open_in_browser(),
            Action::Copy => self.copy_number(),
            Action::CopyLink => self.copy_link(),
            Action::Sort => self.open_sort_picker(),
            Action::Filter => match self.current_item() {
                _ if self.focus == Panel::Builds => self.open_build_filter(),
                Some(Detail::Build(_)) => self.open_build_filter(),
                Some(Detail::PullRequest(_)) => self.open_pr_filter(),
                _ => self.load_work_item_types(),
            },
            Action::Sprint => self.load_sprints(),
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
    use crate::api::{Mention, PullRequestDetails};
    use crate::config::WorkItemSort;
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
            description: None,
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
    fn paste_into_comment_keeps_lines_and_triggers_no_actions() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: String::new(),
        });

        app.handle_paste("# Title\r\n\r\n- q item\r- j item\n```rust\nfn x() {}\n```");

        assert!(!app.should_quit);
        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { text, .. })
                if text == "# Title\n\n- q item\n- j item\n```rust\nfn x() {}\n```"
        ));
    }

    #[test]
    fn paste_into_setup_input_takes_first_line() {
        let mut app = app();
        press(&mut app, KeyCode::Char('T'));

        app.handle_paste("token\nq");

        assert!(!app.should_quit);
        assert!(
            matches!(&app.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input == "token")
        );
    }

    #[test]
    fn paste_without_input_is_ignored() {
        let mut app = app();

        app.handle_paste("q");

        assert!(!app.should_quit);
        assert!(app.popup.is_none());
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
    fn toolbar_offers_down_for_unassigned_items() {
        let mut app = app();
        ctrl(&mut app, 'j');

        assert!(app.actions().contains(&Action::AssignToMe));
        assert!(app.actions().contains(&Action::Down));
    }

    #[test]
    fn signals_are_stored_for_my_pull_requests() {
        let mut app = app();
        let details = PullRequestDetails {
            description: None,
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

    fn mention(id: u32) -> Mention {
        Mention {
            item: work_item(id, "Mentioned", "Bug", "Active"),
            date: "2026-10-01T10:00:00Z".into(),
        }
    }

    fn notifying_app(notifications: bool) -> App {
        let mut config = config();
        config.notifications = notifications;
        let mut app = App::new(Some(config), None);
        app.screen = Screen::Main;
        app
    }

    fn keys(details: &[Detail]) -> Vec<String> {
        details.iter().map(Detail::key).collect()
    }

    /// Loads the inbox with mentions and starts tracking it, like the second refresh.
    fn loaded(app: &mut App, ids: &[u32]) {
        app.handle_message(Message::Mentions(Ok(ids
            .iter()
            .map(|id| mention(*id))
            .collect())));
        app.last_updated = Some(Instant::now());
        app.track_inbox();
    }

    #[test]
    fn first_load_is_not_notified() {
        let mut app = notifying_app(true);

        app.handle_message(Message::Mentions(Ok(vec![mention(20)])));
        let first = app.new_inbox_entries();
        app.last_updated = Some(Instant::now());
        app.track_inbox();
        let after_tracking = app.new_inbox_entries();

        assert!(first.is_empty());
        assert!(after_tracking.is_empty());
    }

    #[test]
    fn new_inbox_entry_is_notified_once() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let first = app.new_inbox_entries();
        let second = app.new_inbox_entries();

        assert_eq!(keys(&first), ["#21"]);
        assert!(second.is_empty());
    }

    #[test]
    fn notified_entry_is_not_notified_after_refresh() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        app.new_inbox_entries();

        app.track_inbox();
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn returning_entry_is_not_notified_again() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(21)])));
        app.new_inbox_entries();
        app.handle_message(Message::Mentions(Ok(Vec::new())));
        app.new_inbox_entries();

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn nothing_is_notified_when_notifications_are_off() {
        let mut app = notifying_app(false);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }
}
