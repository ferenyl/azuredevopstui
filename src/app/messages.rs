use std::time::Instant;

use crate::api;
use crate::api::AdoClient;
use crate::auth::Auth;

use super::message::Message;
use super::{App, DetailInfo, Popup, Retry, Screen, Selection, SetupStep, Signals, SprintPicker};

impl App {
    pub(super) fn handle_message(&mut self, message: Message) {
        match message {
            Message::Notified(url) => {
                if let Some(url) = url {
                    self.open_url(&url);
                }
            }
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
            Message::Sprints(result) => match result {
                Ok(sprints) => {
                    let picker = SprintPicker::new(sprints, self.sprint.as_deref());
                    self.popup = Some(Popup::Sprint(picker));
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
            tokio::spawn(Auth::forget_pat());
            self.set_step(SetupStep::EnterPat {
                input: String::new(),
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

    use crate::api::PullRequestDetails;
    use crate::app::Detail;
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

        assert_eq!(ids_of_prs(app.data.my_prs.as_ref().unwrap()), [2, 1]);
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
}
