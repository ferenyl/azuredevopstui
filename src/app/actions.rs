use super::message::Message;
use super::{App, Detail};
use crate::api;
use crate::api::{PullRequest, WorkItem};
use crossterm::clipboard::CopyToClipboard;
use std::process::{Command, Stdio};
use std::time::Instant;

impl App {
    pub(super) fn current_url(&self) -> Option<String> {
        self.detail_url(&self.current_item()?)
    }

    pub(super) fn detail_url(&self, item: &Detail) -> Option<String> {
        let config = self.config.as_ref()?;
        let url = match item {
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

    pub(super) fn open_in_browser(&mut self) {
        if let Some(url) = self.current_url() {
            self.open_url(&url);
        }
    }

    pub(super) fn open_url(&mut self, url: &str) {
        let Some(config) = &self.config else {
            return;
        };
        let result = match &config.browser_command {
            Some(command) => {
                let mut parts = command.split_whitespace();
                match parts.next() {
                    Some(program) => Command::new(program)
                        .args(parts)
                        .arg(url)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
            None => open::that_detached(url),
        };
        if let Err(err) = result {
            tracing::warn!("failed to open browser: {err}");
            self.error = Some(format!("failed to open browser: {err}"));
        }
    }

    pub(super) fn copy_number(&mut self) {
        let id = match self.current_item() {
            Some(Detail::PullRequest(pr)) => format!("!{}", pr.pull_request_id),
            Some(Detail::WorkItem(item)) => format!("#{}", item.id),
            Some(Detail::Build(_)) | None => return,
        };
        self.copy(id);
    }

    pub(super) fn copy_link(&mut self) {
        if let Some(url) = self.current_url() {
            self.copy(url);
        }
    }

    pub(super) fn copy(&mut self, text: String) {
        let copy = CopyToClipboard::to_clipboard_from(text.as_str());
        match crossterm::execute!(std::io::stdout(), copy) {
            Ok(()) => self.notice = Some((format!("copied {text}"), Instant::now())),
            Err(err) => self.error = Some(format!("failed to copy: {err}")),
        }
    }

    pub(super) fn assign_to_me(&mut self) {
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

    pub(super) fn is_assigned_to_me(&self, item: &WorkItem) -> bool {
        let assignee = item.fields.assigned_to.as_ref().map(|a| a.id.as_str());
        self.user
            .as_ref()
            .is_some_and(|user| assignee == Some(user.id.as_str()))
    }

    /// Asks for the new column first; nothing changes until it is confirmed.
    pub(super) fn unassign(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if self.is_assigned_to_me(&item) {
            self.change_column(true);
        } else {
            self.error = Some(format!("#{} is not assigned to you", item.id));
        }
    }

    pub(super) fn tag(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if !self.is_assigned_to_me(&item) {
            self.error = Some(format!("#{} is not assigned to you", item.id));
            return;
        }
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            let result = client.tags(&config.organization, &config.project).await;
            Message::Tags { item, result }
        });
    }

    pub(super) fn change_column(&mut self, unassign: bool) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        if let Some(board) = &self.board {
            let board = board.clone();
            self.open_column_picker(item, &board, unassign);
            return;
        }
        let Some((client, config)) = self.connection() else {
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

    /// My PR with nothing blocking the merge.
    pub(super) fn can_complete(&self, pr: &PullRequest) -> bool {
        self.data
            .pr_signals
            .get(&pr.pull_request_id)
            .is_some_and(|signals| signals.ready)
    }

    /// Asks for the merge strategy first; nothing is completed until it is confirmed.
    pub(super) fn complete(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        if !self.can_complete(&pr) {
            self.error = Some(format!("!{} is not ready to complete", pr.pull_request_id));
            return;
        }
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            let result = client
                .merge_strategies(&config.organization, &config.project, &pr)
                .await;
            Message::MergeStrategies { pr, result }
        });
    }

    /// Column names of the team board, empty until it is loaded.
    pub fn board_columns(&self) -> Vec<String> {
        self.board
            .iter()
            .flat_map(|board| &board.columns)
            .map(|column| column.name.clone())
            .collect()
    }
}
