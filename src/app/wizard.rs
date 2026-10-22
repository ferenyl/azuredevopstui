use crate::auth::Auth;
use crate::config::{AuthMethod, Config};

use super::message::Message;
use super::setup::organization_name;
use super::{App, Retry, Screen, SetupStep};

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
                self.config = Some(config);
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

    pub(super) fn confirm_setup_step(&mut self) {
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
