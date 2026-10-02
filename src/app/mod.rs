mod action;
mod message;
mod setup;
mod state;

pub use action::Action;
pub use setup::{Retry, Selection, SetupStep};
pub use state::{Data, Detail, DetailInfo, LEFT_PANELS, Panel};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::api::{self, AdoClient, CurrentUser};
use crate::auth::Auth;
use crate::config::{AuthMethod, Config};
use crate::theme::Theme;
use message::Message;

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
    last_left: Panel,
    pub error: Option<String>,
    client: Option<AdoClient>,
    tx: UnboundedSender<Message>,
    rx: UnboundedReceiver<Message>,
    should_quit: bool,
}

impl App {
    pub fn new(config: Option<Config>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            theme: config
                .as_ref()
                .map(|c| c.colors.clone())
                .unwrap_or_default(),
            config,
            screen: Screen::Setup(SetupStep::Loading("Signing in…")),
            focus: Panel::MyPrs,
            user: None,
            data: Data::default(),
            selections: [0; LEFT_PANELS.len()],
            detail: None,
            detail_info: None,
            detail_scroll: 0,
            last_left: Panel::MyPrs,
            error: None,
            client: None,
            tx,
            rx,
            should_quit: false,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        self.authenticate();
        let mut events = EventStream::new();
        while !self.should_quit {
            terminal.draw(|frame| crate::ui::render(frame, &self))?;
            tokio::select! {
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(err.into()),
                    None => break,
                },
                Some(message) = self.rx.recv() => self.handle_message(message),
            }
        }
        Ok(())
    }

    pub fn actions(&self) -> &'static [Action] {
        match &self.screen {
            Screen::Setup(step) if step.is_input() => &[Action::Confirm, Action::Cancel],
            Screen::Setup(SetupStep::Failed { .. }) => &[Action::Reload, Action::Quit],
            Screen::Setup(step) if step.selection().is_some() => {
                &[Action::Down, Action::Up, Action::Confirm, Action::Quit]
            }
            Screen::Setup(_) => &[Action::Quit],
            Screen::Main => &[
                Action::Down,
                Action::FocusRight,
                Action::Confirm,
                Action::Reload,
                Action::Quit,
            ],
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
            });
        }
        pat_rejected
    }

    fn fail(&mut self, err: anyhow::Error, retry: Retry) {
        if self.handle_rejected_pat(&err) {
            return;
        }
        self.set_step(SetupStep::Failed {
            error: format!("{err:#}"),
            retry,
        });
    }

    fn report_error(&mut self, err: anyhow::Error) {
        if !self.handle_rejected_pat(&err) {
            self.error = Some(format!("{err:#}"));
        }
    }

    fn refresh(&mut self) {
        let (Some(client), Some(config), Some(user)) =
            (self.client.clone(), self.config.clone(), self.user.clone())
        else {
            return;
        };
        self.error = None;

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::MyPullRequests(
                c.my_pull_requests(&cfg.organization, &cfg.project, &user_id)
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

        if let Some(ready_column) = config.ready_column.clone() {
            self.spawn(async move {
                Message::SprintWorkItems(
                    client
                        .sprint_work_items(
                            &config.organization,
                            &config.project,
                            &config.team,
                            &ready_column,
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

    fn authenticate(&mut self) {
        let method = self
            .config
            .as_ref()
            .map(|c| c.auth.method)
            .unwrap_or(AuthMethod::Auto);
        self.spawn(async move { Message::Authenticated(Auth::resolve(method).await) });
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
            Message::Connected(Ok(user)) => {
                self.user = Some(user);
                self.screen = Screen::Main;
                self.refresh();
            }
            Message::MyPullRequests(result) => match result {
                Ok(prs) => self.data.my_prs = Some(prs),
                Err(err) => self.report_error(err),
            },
            Message::OtherPullRequests(result) => match result {
                Ok(prs) => self.data.other_prs = Some(prs),
                Err(err) => self.report_error(err),
            },
            Message::SprintWorkItems(result) => match result {
                Ok(sprint) => {
                    self.data.sprint_name = Some(sprint.iteration_name);
                    self.data.my_work_items = Some(sprint.mine);
                    self.data.ready_work_items = Some(sprint.ready);
                }
                Err(err) => self.report_error(err),
            },
            Message::Connected(Err(err)) => self.fail(err, Retry::Continue),
            Message::PullRequestDetails { id, result } => {
                if self.detail.as_ref().is_some_and(|d| d.is_pull_request(id)) {
                    match result {
                        Ok(info) => self.detail_info = Some(DetailInfo::PullRequest(info)),
                        Err(err) => self.report_error(err),
                    }
                }
            }
            Message::WorkItemDetails { id, result } => {
                if self.detail.as_ref().is_some_and(|d| d.is_work_item(id)) {
                    match result {
                        Ok(info) => self.detail_info = Some(DetailInfo::WorkItem(info)),
                        Err(err) => self.report_error(err),
                    }
                }
            }
        }
        self.clamp_selections();
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
        let (Some(index), Some(client), Some(config)) =
            (self.focus.index(), self.client.clone(), self.config.clone())
        else {
            return;
        };
        let Some(detail) = self.data.detail(self.focus, self.selections[index]) else {
            return;
        };
        match &detail {
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
        }
        self.detail = Some(detail);
        self.detail_info = None;
        self.detail_scroll = 0;
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
        if let Some(action) = Action::from_key(key) {
            self.apply(action);
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Cancel => {
                if matches!(self.screen, Screen::Setup(_)) {
                    self.should_quit = true;
                }
            }
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
            SetupStep::EnterOrganization(organization) if !organization.trim().is_empty() => {
                self.load_projects(organization.trim().to_string());
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
