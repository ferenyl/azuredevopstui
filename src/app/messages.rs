use std::time::Instant;

use crate::api;
use crate::api::AdoClient;
use crate::auth::Auth;

use super::loads::Load;
use super::message::Message;
use super::{
    App, DetailInfo, Popup, Retry, Screen, Selection, SetupStep, Signals, SprintPicker, TextInput,
};

impl App {
    pub(super) fn handle_message(&mut self, message: Message) {
        let message = match message {
            Message::Loaded {
                load,
                generation,
                message,
            } => {
                if !self.loads.finish(load, generation) {
                    tracing::debug!(?load, generation, "dropped stale response");
                    return;
                }
                *message
            }
            message => message,
        };
        match message {
            Message::Loaded { .. } => {}
            Message::Notified(url) => {
                if let Some(url) = url {
                    self.open_url(&url);
                }
            }
            Message::Authenticated(Ok(Some(auth))) => {
                self.client = Some(AdoClient::new(auth));
                self.sync_api();
                self.continue_setup();
            }
            Message::Authenticated(Ok(None)) => self.set_step(SetupStep::EnterPat {
                input: TextInput::default(),
                error: None,
                cancelable: false,
            }),
            Message::Authenticated(Err(err)) => self.fail(err, Retry::Authenticate),
            Message::Organizations(Ok(organizations)) if !organizations.is_empty() => {
                self.set_step(SetupStep::SelectOrganization(Selection::new(organizations)));
            }
            Message::Organizations(_) => {
                self.set_step(SetupStep::EnterOrganization(TextInput::default()));
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
                    self.data.set_my_prs(prs);
                    self.last_updated = Some(Instant::now());
                }
                Err(err) => self.report_error(err),
            },
            Message::OtherPullRequests(result) => match result {
                Ok(prs) => {
                    self.data.set_other_prs(prs.items);
                    self.note_failures(
                        prs.failed,
                        "PR without review signals",
                        "PRs without review signals",
                    );
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
            Message::Sprints(result) => match result {
                Ok(sprints) => {
                    let picker = SprintPicker::new(sprints, self.sprint.as_deref());
                    self.popup = Some(Popup::Sprint(picker));
                }
                Err(err) => self.report_error(err),
            },
            Message::Builds(result) => match result {
                Ok(builds) => self.data.set_builds(builds),
                Err(err) => self.report_error(err),
            },
            Message::TeamBoard(result) => match result {
                Ok(board) => self.board = Some(board),
                Err(err) => tracing::warn!("failed to load board: {err:#}"),
            },
            Message::Mentions(result) => match result {
                Ok(mentions) => {
                    self.data.set_mentions(mentions.items);
                    self.note_failures(
                        mentions.failed,
                        "work item skipped without comments",
                        "work items skipped without comments",
                    );
                }
                Err(err) => self.report_error(err),
            },
            Message::MergeStrategies { pr, result } => match result {
                Ok(strategies) => self.open_complete_picker(pr, strategies),
                Err(err) => self.report_error(err),
            },
            Message::PullRequestCompleted { id, result } => match result {
                Ok(()) => {
                    tracing::info!(id, "pull request completed");
                    self.reload_only(&[
                        Load::MyPullRequests,
                        Load::OtherPullRequests,
                        Load::Detail,
                    ]);
                }
                Err(err) => self.report_error(err),
            },
            Message::PullRequestUpdated { id, reload, result } => match result {
                Ok(()) => {
                    tracing::info!(id, "pull request updated");
                    let shown = self.detail.as_ref().is_some_and(|d| d.is_pull_request(id));
                    let loads: Vec<Load> = reload
                        .into_iter()
                        .filter(|load| *load != Load::Detail || shown)
                        .collect();
                    self.reload_only(&loads);
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
                    .my_prs()
                    .into_iter()
                    .flatten()
                    .find(|pr| pr.pull_request_id == id);
                match (pr, result) {
                    (Some(pr), Ok(checks)) => {
                        let signals = Signals::new(pr, &checks);
                        self.data.set_pr_signals(id, signals);
                    }
                    (_, Err(err)) => tracing::warn!(id, "failed to load PR signals: {err:#}"),
                    (None, _) => {}
                }
            }
            Message::Image { url, result } => self.images.insert(url, result),
            Message::WorkItemUpdated { id, reload, result } => match result {
                Ok(()) => {
                    tracing::info!(id, "work item updated");
                    let shown = self.detail.as_ref().is_some_and(|d| d.is_work_item(id));
                    let loads: Vec<Load> = reload
                        .iter()
                        .copied()
                        .filter(|load| *load != Load::Detail || shown)
                        .collect();
                    self.reload_only(&loads);
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

    pub(super) fn fail(&mut self, err: anyhow::Error, retry: Retry) {
        tracing::error!("setup failed: {err:#}");
        if self.handle_rejected_pat(&err) {
            return;
        }
        self.set_step(SetupStep::Failed {
            error: format!("{err:#}"),
            retry,
        });
    }

    pub(super) fn report_error(&mut self, err: anyhow::Error) {
        tracing::error!("{err:#}");
        if !self.handle_rejected_pat(&err) {
            self.error = Some(format!("{err:#}"));
        }
    }

    /// Drops a rejected PAT and asks for a new one. Returns true when handled.
    pub(super) fn handle_rejected_pat(&mut self, err: &anyhow::Error) -> bool {
        let pat_rejected =
            api::is_unauthorized(err) && self.client.as_ref().is_some_and(|c| c.auth().uses_pat());
        if pat_rejected {
            self.client = None;
            self.sync_api();
            tokio::spawn(Auth::forget_pat());
            self.set_step(SetupStep::EnterPat {
                input: TextInput::default(),
                error: Some("The saved token was rejected. Enter a new one.".into()),
                cancelable: false,
            });
        }
        pat_rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::{PullRequestChecks, PullRequestDetails};
    use crate::app::Detail;
    use crate::app::loads::Load;
    use crate::app::message::Message;
    use crate::app::test_helpers::{app, open_pull_request};
    use crate::test_support::{ids_of_prs, pull_request};

    #[test]
    fn loaded_lists_use_configured_sort() {
        let mut app = app();

        app.handle_message(Message::MyPullRequests(Ok(vec![
            pull_request(1, "Old", "me", "2026-10-01T10:00:00Z"),
            pull_request(2, "New", "me", "2026-10-05T10:00:00Z"),
        ])));

        assert_eq!(ids_of_prs(app.data.my_prs().unwrap()), [2, 1]);
        assert!(app.last_updated.is_some());
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
    fn signals_are_stored_for_my_pull_requests() {
        let mut app = app();
        let checks = PullRequestChecks {
            threads: serde_json::from_value(serde_json::json!([
                { "status": "active", "comments": [] }
            ]))
            .unwrap(),
            statuses: Vec::new(),
            policies: Vec::new(),
            has_work_items: false,
        };

        app.handle_message(Message::PullRequestSignals {
            id: 2,
            result: Ok(checks),
        });

        assert_eq!(app.data.pr_signals()[&2].unresolved, 1);
    }

    #[test]
    fn signals_for_removed_pull_requests_are_dropped() {
        let mut app = app();
        app.data.set_pr_signals(3, Signals::default());

        app.handle_message(Message::MyPullRequests(Ok(vec![pull_request(
            1,
            "Only",
            "me",
            "2026-10-01T10:00:00Z",
        )])));

        assert!(!app.data.pr_signals().contains_key(&3));
    }

    fn sprint(name: &str) -> Message {
        Message::SprintWorkItems(Ok(crate::api::SprintWorkItems {
            iteration_name: name.into(),
            start_date: None,
            finish_date: None,
            mine: Vec::new(),
            ready: Vec::new(),
        }))
    }

    fn start(app: &mut App, load: Load) -> u64 {
        let generation = app.loads.next(load);
        let task = tokio::spawn(std::future::pending::<()>());
        app.loads.track(load, generation, task.abort_handle());
        generation
    }

    fn loaded(load: Load, generation: u64, message: Message) -> Message {
        Message::Loaded {
            load,
            generation,
            message: Box::new(message),
        }
    }

    #[tokio::test]
    async fn response_for_an_older_sprint_is_dropped() {
        let mut app = app();
        let old = start(&mut app, Load::Sprint);
        let new = start(&mut app, Load::Sprint);

        app.handle_message(loaded(Load::Sprint, new, sprint("Sprint 2")));
        app.handle_message(loaded(Load::Sprint, old, sprint("Sprint 1")));

        assert_eq!(app.data.sprint_name.as_deref(), Some("Sprint 2"));
        assert!(!app.loads.is_loading(Load::Sprint));
    }

    #[tokio::test]
    async fn responses_after_the_project_changes_are_dropped() {
        let mut app = app();
        let old = start(&mut app, Load::Sprint);

        app.sync_api();
        app.handle_message(loaded(Load::Sprint, old, sprint("Old project")));

        assert_eq!(app.data.sprint_name.as_deref(), Some("Sprint 1"));
    }
}
