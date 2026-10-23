use std::sync::Arc;

use crate::api::ProjectApi;
use crate::auth::Auth;
use crate::config::{AuthMethod, Config};
use crate::theme::Theme;

use super::loads::Load;
use super::message::Message;
use super::setup::organization_name;
use super::{App, Data, LEFT_PANELS, Panel, Retry, Screen, SetupStep};

impl App {
    pub(super) fn authenticate(&mut self) {
        let auth = self
            .config
            .as_ref()
            .map(|c| c.auth.clone())
            .unwrap_or_default();
        self.spawn(async move { Message::Authenticated(Auth::resolve(auth).await) });
    }

    pub(super) fn continue_setup(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        match &self.config {
            None => self.load_organizations(),
            Some(config) if config.ready_column.is_none() => {
                self.load_board_columns((**config).clone());
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

    pub(super) fn load_organizations(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.set_step(SetupStep::Loading("Loading organizations…"));
        self.spawn(async move { Message::Organizations(client.organizations().await) });
    }

    /// Loads the columns of the board of the team in `config`, which is saved once one is picked.
    pub(super) fn load_board_columns(&mut self, config: Config) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.set_step(SetupStep::Loading("Loading board columns…"));
        let api = ProjectApi::for_team(client, &config.organization, &config.project, &config.team);
        self.spawn(async move {
            Message::BoardColumns {
                result: api.board_columns().await,
                config: Box::new(config),
            }
        });
    }

    /// Reopens the setup from the main view; cancelling keeps the current project.
    pub(super) fn switch_project(&mut self) {
        self.switching = true;
        self.setup_back.clear();
        self.load_organizations();
    }

    /// Esc: back one step; from the first step cancel a switch or quit.
    pub(super) fn go_back(&mut self) {
        let waiting = matches!(self.screen, Screen::Setup(SetupStep::Loading(_)));
        if waiting && (self.switching || !self.setup_back.is_empty()) {
            return;
        }
        match self.setup_back.pop() {
            Some(step) => self.set_step(step),
            None if self.switching => {
                self.switching = false;
                self.screen = Screen::Main;
            }
            None => self.should_quit = true,
        }
    }

    pub(super) fn can_go_back(&self) -> bool {
        self.switching || !self.setup_back.is_empty()
    }

    /// Drops everything of the previous project once the new one is saved.
    fn reset_project(&mut self) {
        self.data = Data::default();
        self.selections = [0; LEFT_PANELS.len()];
        self.close_detail();
        self.board_view = false;
        self.board = None;
        self.loads.cancel(Load::SprintBoard);
        self.notified = None;
        self.popup = None;
        self.search = None;
        self.focus = Panel::Inbox;
        self.last_left = Panel::Inbox;
        self.last_updated = None;
    }

    pub(super) fn load_projects(&mut self, organization: String) {
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

    pub(super) fn load_teams(&mut self, organization: String, project: String) {
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

    pub(super) fn save_config(&mut self, config: Config) {
        match config.save() {
            Ok(()) => {
                self.theme = Theme::from_config(&config);
                self.sprint = config.default_sprint.clone();
                self.config = Some(Arc::new(config));
                self.setup_back.clear();
                if self.switching {
                    self.switching = false;
                    self.reset_project();
                }
                self.sync_api();
                self.continue_setup();
            }
            Err(err) => self.fail(err, Retry::SaveConfig(Box::new(config))),
        }
    }

    pub(super) fn retry(&mut self, retry: Retry) {
        match retry {
            Retry::Authenticate => {
                self.set_step(SetupStep::Loading("Signing in…"));
                self.authenticate();
            }
            Retry::Continue => self.continue_setup(),
            Retry::Projects(organization) => self.load_projects(organization),
            Retry::Teams(organization, project) => self.load_teams(organization, project),
            Retry::BoardColumns(config) => self.load_board_columns(*config),
            Retry::SaveConfig(config) => self.save_config(*config),
        }
    }

    pub(super) fn can_enter_pat(&self) -> bool {
        self.config
            .as_ref()
            .is_none_or(|c| c.auth.method == AuthMethod::Auto)
    }

    pub(super) fn set_step(&mut self, step: SetupStep) {
        self.screen = Screen::Setup(step);
    }

    /// The config for the picked team: the running one when switching, so settings are kept.
    fn picked_config(&self, organization: &str, project: &str, team: &str) -> Config {
        let mut config = match &self.config {
            Some(current) if self.switching => (**current).clone(),
            _ => Config::new(organization.into(), project.into(), team.into()),
        };
        config.organization = organization.into();
        config.project = project.into();
        config.team = team.into();
        config.ready_column = None;
        config.default_sprint = None;
        config
    }

    pub(super) fn confirm_setup_step(&mut self) {
        let Screen::Setup(step) = std::mem::replace(&mut self.screen, Screen::Main) else {
            return;
        };
        match step {
            SetupStep::EnterPat { input, .. } if !input.as_str().trim().is_empty() => {
                let pat = input.as_str().trim().to_string();
                self.set_step(SetupStep::Loading("Saving token…"));
                self.spawn(async move {
                    Message::Authenticated(Auth::with_new_pat(pat).await.map(Some))
                });
            }
            SetupStep::EnterOrganization(input)
                if !organization_name(input.as_str()).is_empty() =>
            {
                let organization = organization_name(input.as_str()).to_string();
                self.setup_back.push(SetupStep::EnterOrganization(input));
                self.load_projects(organization);
            }
            SetupStep::SelectOrganization(selection) => match selection.current() {
                Some(organization) => {
                    let organization = organization.to_string();
                    self.setup_back
                        .push(SetupStep::SelectOrganization(selection));
                    self.load_projects(organization);
                }
                None => self.set_step(SetupStep::SelectOrganization(selection)),
            },
            SetupStep::SelectProject {
                organization,
                selection,
            } => match selection.current() {
                Some(project) => {
                    let project = project.to_string();
                    self.setup_back.push(SetupStep::SelectProject {
                        organization: organization.clone(),
                        selection,
                    });
                    self.load_teams(organization, project);
                }
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
                    let config = self.picked_config(&organization, &project, team);
                    self.setup_back.push(SetupStep::SelectTeam {
                        organization,
                        project,
                        selection,
                    });
                    self.load_board_columns(config);
                }
                None => self.set_step(SetupStep::SelectTeam {
                    organization,
                    project,
                    selection,
                }),
            },
            SetupStep::SelectReadyColumn { config, selection } => match selection.current() {
                Some(column) => {
                    let mut config = *config;
                    config.ready_column = Some(column.to_string());
                    self.save_config(config);
                }
                None => self.set_step(SetupStep::SelectReadyColumn { config, selection }),
            },
            step => self.set_step(step),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::app::test_helpers::{app, press};
    use crate::test_support::config;

    fn setup_app() -> App {
        App::new(None, None)
    }

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    fn current(app: &App) -> Option<&str> {
        match &app.screen {
            Screen::Setup(step) => step.selection().and_then(|s| s.current()),
            Screen::Main => None,
        }
    }

    #[test]
    fn esc_goes_back_to_the_previous_step_with_its_selection() {
        let mut app = setup_app();
        app.handle_message(Message::Organizations(Ok(names(&["a", "b"]))));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        app.handle_message(Message::Projects {
            organization: "b".into(),
            result: Ok(names(&["p", "q"])),
        });
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        app.handle_message(Message::Teams {
            organization: "b".into(),
            project: "q".into(),
            result: Ok(names(&["t"])),
        });

        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            &app.screen,
            Screen::Setup(SetupStep::SelectProject { organization, .. }) if organization == "b"
        ));
        assert_eq!(current(&app), Some("q"));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::SelectOrganization(_))
        ));
        assert_eq!(current(&app), Some("b"));
        assert!(!app.should_quit);
    }

    #[test]
    fn esc_from_the_ready_column_goes_back_to_the_team() {
        let mut app = setup_app();
        app.handle_message(Message::Teams {
            organization: "o".into(),
            project: "p".into(),
            result: Ok(names(&["t"])),
        });
        press(&mut app, KeyCode::Enter);
        app.handle_message(Message::BoardColumns {
            config: Box::new(config()),
            result: Ok(names(&["Ready"])),
        });
        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::SelectReadyColumn { .. })
        ));
        assert!(app.config.is_none());

        press(&mut app, KeyCode::Esc);

        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::SelectTeam { .. })
        ));
        assert!(app.config.is_none());
    }

    #[test]
    fn esc_from_a_failure_goes_back() {
        let mut app = setup_app();
        app.handle_message(Message::Organizations(Ok(names(&["a"]))));
        press(&mut app, KeyCode::Enter);
        app.handle_message(Message::Projects {
            organization: "a".into(),
            result: Err(anyhow::anyhow!("boom")),
        });
        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::Failed { .. })
        ));

        press(&mut app, KeyCode::Esc);

        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::SelectOrganization(_))
        ));
    }

    #[test]
    fn esc_on_the_first_step_quits() {
        let mut app = setup_app();
        app.handle_message(Message::Organizations(Ok(names(&["a"]))));

        press(&mut app, KeyCode::Esc);

        assert!(app.should_quit);
    }

    #[test]
    fn esc_on_the_first_step_of_a_switch_returns_to_the_main_view() {
        let mut app = app();
        app.switching = true;
        app.handle_message(Message::Organizations(Ok(names(&["contoso"]))));

        press(&mut app, KeyCode::Esc);

        assert!(matches!(app.screen, Screen::Main));
        assert!(!app.switching);
        assert!(!app.should_quit);
        assert!(app.data.my_prs().is_some());
    }

    #[test]
    fn switching_preselects_the_current_values() {
        let mut app = app();
        app.switching = true;

        app.handle_message(Message::Organizations(Ok(names(&["alpha", "contoso"]))));
        assert_eq!(current(&app), Some("contoso"));
        app.handle_message(Message::Projects {
            organization: "contoso".into(),
            result: Ok(names(&["Other", "MyProject"])),
        });
        assert_eq!(current(&app), Some("MyProject"));
        app.handle_message(Message::Teams {
            organization: "contoso".into(),
            project: "MyProject".into(),
            result: Ok(names(&["A", "My Team"])),
        });
        assert_eq!(current(&app), Some("My Team"));
        app.handle_message(Message::Teams {
            organization: "contoso".into(),
            project: "Other".into(),
            result: Ok(names(&["A", "My Team"])),
        });
        assert_eq!(current(&app), Some("A"));
    }

    #[test]
    fn picked_config_keeps_settings_when_switching() {
        let mut app = app();
        let mut current = config();
        current.browser_command = Some("firefox".into());
        current.default_sprint = Some("Sprint 1".into());
        app.config = Some(Arc::new(current));
        app.switching = true;

        let picked = app.picked_config("contoso", "Other", "Mobile");

        assert_eq!(picked.project, "Other");
        assert_eq!(picked.team, "Mobile");
        assert_eq!(picked.browser_command.as_deref(), Some("firefox"));
        assert_eq!(picked.ready_column, None);
        assert_eq!(picked.default_sprint, None);
    }

    #[test]
    fn picked_config_is_new_on_first_setup() {
        let app = setup_app();

        let picked = app.picked_config("o", "p", "t");

        assert_eq!(
            (picked.organization.as_str(), picked.team.as_str()),
            ("o", "t")
        );
        assert_eq!(picked.ready_column, None);
    }

    #[test]
    fn saving_a_switched_config_resets_the_previous_project() {
        let mut app = app();
        app.switching = true;
        app.selections[0] = 2;
        let mut new = config();
        new.team = "Mobile".into();

        app.save_config(new);

        assert!(!app.switching);
        assert_eq!(app.config.as_ref().unwrap().team, "Mobile");
        assert!(app.data.my_prs().is_none());
        assert_eq!(app.selections, [0; LEFT_PANELS.len()]);
        assert!(app.setup_back.is_empty());
    }

    #[test]
    fn cancelling_a_switch_keeps_the_running_project() {
        let mut app = app();
        app.config = Some(Arc::new(config()));
        app.switch_project();
        assert!(app.switching);

        press(&mut app, KeyCode::Esc);

        assert!(matches!(app.screen, Screen::Main));
        assert_eq!(app.config.as_ref().unwrap().project, "MyProject");
        assert!(app.data.my_prs().is_some());
    }
}
