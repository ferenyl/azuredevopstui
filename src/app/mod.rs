mod action;
mod actions;
mod activity;
#[cfg(test)]
mod e2e;
mod editor;
mod files;
mod inbox;
mod input;
mod keymap;
mod loads;
mod message;
mod messages;
mod mouse;
mod popups;
mod refresh;
mod setup;
mod state;
#[cfg(test)]
mod test_helpers;
mod text_input;
mod toolbar;
mod wizard;

pub use action::{Action, HELP};
pub use activity::Activity;
pub use files::Files;
pub use keymap::Keymap;
pub use mouse::{HitAreas, PanelHit, PickerHit};
pub use setup::{Retry, Selection, SetupStep};
pub use state::{
    BuildAction, ColumnPicker, CompletePicker, CompleteRow, ConfirmAction, Data, Detail,
    DetailInfo, DetailTab, InboxEntry, LEFT_PANELS, Panel, Popup, PrAction, Signals, SortKind,
    SprintPicker, TagPicker, TextTarget,
};
pub use text_input::TextInput;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::Rect;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::api::{AdoClient, Board, CurrentUser, ProjectApi, WorkItem};
use crate::config::Config;
use crate::images::Images;
use crate::terminal::TerminalModes;
use crate::theme::Theme;
use loads::{Load, Loads};
use message::Message;

const NOTICE_DURATION: Duration = Duration::from_secs(3);

pub enum Screen {
    Setup(SetupStep),
    Main,
}

pub struct App {
    pub config: Option<Arc<Config>>,
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
    /// Selected thread, as an index into the threads in listed order.
    pub detail_thread: Option<usize>,
    /// The next scroll starts at the selected thread; drawn by the UI, which knows the rows.
    pub scroll_to_thread: bool,
    /// Row of the selected thread in the last drawn detail.
    pub thread_row: std::cell::Cell<Option<u16>>,
    /// Largest useful `detail_scroll` of the last drawn detail; unbounded before the first draw.
    pub detail_max_scroll: std::cell::Cell<u16>,
    /// Visible content rows of the last drawn detail.
    pub detail_height: std::cell::Cell<u16>,
    /// Visible rows of the focused left list in the last frame; ten before the first draw.
    pub list_height: std::cell::Cell<u16>,
    /// Changed files of the shown PR, once its Files tab was shown.
    pub files: Option<Files>,
    /// Targets of the links drawn in the detail pane last frame, in reading order.
    pub detail_links: RefCell<Vec<String>>,
    /// Selected entry of `detail_links`.
    pub link_index: Option<usize>,
    /// Every URL `open_url` was asked to open, so tests need no browser.
    #[cfg(test)]
    opened: Vec<String>,
    pub popup: Option<Popup>,
    /// Confirmed setup steps, most recent last; esc goes back to the last one.
    setup_back: Vec<SetupStep>,
    /// The setup was opened from the main view to change organization, project or team.
    switching: bool,
    /// Text of the popup to edit in the editor; the run loop picks it up between frames.
    edit_request: Option<String>,
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
    notifier: crate::notify::Notifier,
    pub images: Images,
    client: Option<AdoClient>,
    /// Built once both the client and the config exist.
    api: Option<ProjectApi>,
    loads: Loads,
    tx: UnboundedSender<Message>,
    rx: UnboundedReceiver<Message>,
    should_quit: bool,
    /// The focused panel fills the left column, or the whole screen when it is the detail.
    pub zoomed: bool,
    /// The left panel whose search is being typed, with its text.
    pub search: Option<(Panel, TextInput)>,
    /// Where panels, tabs and the open list were drawn last frame, for mouse hit testing.
    pub areas: RefCell<HitAreas>,
    /// Mutations in flight and the history of errors.
    pub activity: Activity,
    pub keymap: Keymap,
}

impl App {
    pub fn new(config: Option<Config>, picker: Option<ratatui_image::picker::Picker>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            theme: config.as_ref().map(Theme::from_config).unwrap_or_default(),
            sprint: config.as_ref().and_then(|c| c.default_sprint.clone()),
            // `Config::load` already rejected invalid keys.
            keymap: config
                .as_ref()
                .and_then(|c| Keymap::new(&c.keys).ok())
                .unwrap_or_default(),
            config: config.map(Arc::new),
            screen: Screen::Setup(SetupStep::Loading("Signing in…")),
            focus: Panel::Inbox,
            user: None,
            data: Data::default(),
            selections: [0; LEFT_PANELS.len()],
            detail: None,
            detail_info: None,
            detail_scroll: 0,
            detail_tab: DetailTab::default(),
            detail_thread: None,
            scroll_to_thread: false,
            thread_row: std::cell::Cell::new(None),
            detail_max_scroll: std::cell::Cell::new(u16::MAX),
            detail_height: std::cell::Cell::new(0),
            list_height: std::cell::Cell::new(10),
            files: None,
            detail_links: RefCell::default(),
            link_index: None,
            #[cfg(test)]
            opened: Vec::new(),
            popup: None,
            setup_back: Vec::new(),
            switching: false,
            edit_request: None,
            show_help: false,
            last_updated: None,
            last_refresh: Instant::now(),
            board: None,
            last_left: Panel::Inbox,
            error: None,
            notice: None,
            notified: None,
            notifier: Default::default(),
            images: Images::new(picker),
            client: None,
            api: None,
            loads: Loads::default(),
            tx,
            rx,
            should_quit: false,
            zoomed: false,
            search: None,
            areas: RefCell::default(),
            activity: Activity::default(),
        }
    }

    pub async fn run(
        mut self,
        terminal: &mut DefaultTerminal,
        modes: &mut TerminalModes,
    ) -> anyhow::Result<()> {
        self.authenticate();
        let mut events = EventStream::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(200));
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
                    Some(Ok(Event::Mouse(mouse))) => self.handle_mouse(mouse),
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
            if let Some(text) = self.edit_request.take() {
                // The event stream would take the editor's input.
                drop(events);
                modes.suspend();
                self.edit_in_editor(&text);
                modes.resume();
                events = EventStream::new();
                terminal.clear()?;
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

    /// The focused left panel, or the one focus returns to from the detail.
    pub fn left_focus(&self) -> Panel {
        match self.focus {
            Panel::Detail => self.last_left,
            panel => panel,
        }
    }

    fn api(&self) -> Option<ProjectApi> {
        self.api.clone()
    }

    /// Rebuilds the project API after the client or the organization, project or team changes.
    fn sync_api(&mut self) {
        self.loads.cancel_all();
        self.api = match (&self.client, &self.config) {
            (Some(client), Some(config)) => Some(ProjectApi::new(client.clone(), config)),
            _ => None,
        };
    }

    fn note_failures(&mut self, failed: usize, one: &str, many: &str) {
        if failed > 0 {
            let what = if failed == 1 { one } else { many };
            self.notice = Some((format!("{failed} {what}"), Instant::now()));
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

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(future.await);
        });
    }

    /// Like [`App::spawn`] for a change; the toolbar shows `label` until the result arrives.
    fn spawn_action<F>(&mut self, label: String, future: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let id = self.activity.start(label);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let message = Box::new(future.await);
            let _ = tx.send(Message::ActionDone { id, message });
        });
    }

    /// Shows `text` in the toolbar until the next refresh and keeps it in the error history.
    pub(super) fn set_error(&mut self, text: String) {
        self.activity.push_error(text.clone());
        self.error = Some(text);
    }

    /// Spawns the request for `load`, superseding one in flight; only its response is handled.
    fn spawn_load<F>(&mut self, load: Load, future: F)
    where
        F: Future<Output = Message> + Send + 'static,
    {
        let generation = self.loads.next(load);
        let tx = self.tx.clone();
        let task = tokio::spawn(async move {
            let message = Box::new(future.await);
            let _ = tx.send(Message::Loaded {
                load,
                generation,
                message,
            });
        });
        self.loads.track(load, generation, task.abort_handle());
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
    use crate::auth::Auth;
    use crate::test_support::config;

    #[test]
    fn api_needs_both_client_and_config() {
        let mut app = App::new(None, None);
        app.client = Some(AdoClient::new(Auth::from_pat("pat")));
        app.sync_api();
        assert!(app.api().is_none());

        app.config = Some(Arc::new(config()));
        app.sync_api();
        assert!(app.api().is_some());

        app.client = None;
        app.sync_api();
        assert!(app.api().is_none());
    }

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
}
