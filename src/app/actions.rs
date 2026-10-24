use std::process::{Command, Stdio};
use std::time::Instant;

use crossterm::clipboard::CopyToClipboard;

use crate::api;
use crate::api::{PullRequest, WorkItem};

use super::editor::split_command;
use super::loads::Load;
use super::message::Message;
use super::{App, Detail};

impl App {
    pub(super) fn current_url(&self) -> Option<String> {
        self.detail_url(&self.current_item()?)
    }

    pub(super) fn detail_url(&self, item: &Detail) -> Option<String> {
        let config = self.config.as_ref()?;
        let url = match item {
            Detail::PullRequest(pr) => api::pull_request_url(
                &config.organization,
                pr.repository
                    .project
                    .name
                    .as_deref()
                    .unwrap_or(&config.project),
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

    /// The selected link of the detail pane, if it is still shown.
    pub(super) fn selected_link(&self) -> Option<String> {
        self.detail_links.borrow().get(self.link_index?).cloned()
    }

    pub(super) fn select_link(&mut self, next: bool) {
        let count = self.detail_links.borrow().len();
        if count == 0 {
            return;
        }
        self.link_index = Some(match self.link_index {
            None if next => 0,
            None => count - 1,
            Some(index) if next => (index + 1) % count,
            Some(index) => (index + count - 1) % count,
        });
    }

    pub(super) fn open_in_browser(&mut self) {
        if let Some(url) = self.selected_link().or_else(|| self.current_url()) {
            self.open_url(&url);
        }
    }

    pub(super) fn open_url(&mut self, url: &str) {
        #[cfg(test)]
        self.opened.push(url.to_string());
        let Some(config) = &self.config else {
            return;
        };
        let result = match &config.browser_command {
            Some(command) => {
                let mut parts = split_command(command);
                if parts.is_empty() {
                    parts.push(String::new());
                }
                let program = parts.remove(0);
                let has_placeholder = parts.iter().any(|part| part.contains("{url}"));
                let mut args: Vec<String> = parts
                    .iter()
                    .map(|part| part.replace("{url}", url))
                    .collect();
                if !has_placeholder {
                    args.push(url.to_string());
                }
                match program.as_str() {
                    "" => Ok(()),
                    program => Command::new(program)
                        .args(args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map(|_| ()),
                }
            }
            None => open::that_detached(url),
        };
        if let Err(err) = result {
            tracing::warn!("failed to open browser: {err}");
            self.set_error(format!("failed to open browser: {err}"));
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
        if let Some(url) = self.selected_link().or_else(|| self.current_url()) {
            self.copy(url);
        }
    }

    pub(super) fn copy(&mut self, text: String) {
        let configured = self
            .config
            .as_ref()
            .and_then(|c| c.clipboard_command.clone());
        let native = native_copy(configured.as_deref(), &text);
        let osc = crossterm::execute!(
            std::io::stdout(),
            CopyToClipboard::to_clipboard_from(text.as_str())
        );
        match (native, osc) {
            (Some(Ok(())), _) => self.notice = Some((format!("copied {text}"), Instant::now())),
            (Some(Err(err)), _) => self.set_error(format!("failed to copy: {err}")),
            (None, Ok(())) => {
                let notice = format!("sent {text} to the terminal clipboard (OSC 52)");
                self.notice = Some((notice, Instant::now()));
            }
            (None, Err(err)) => self.set_error(format!("failed to copy: {err}")),
        }
    }

    pub(super) fn assign_to_me(&mut self) {
        let (Some(item), Some(api), Some(user)) =
            (self.current_work_item(), self.api(), self.user.as_ref())
        else {
            return;
        };
        let (id, assignee) = (item.id, user.unique_name().to_string());
        self.spawn_action(format!("assigning #{id}"), async move {
            let result = api.assign_work_item(id, &assignee).await;
            Message::WorkItemUpdated {
                id,
                reload: &[Load::Sprint, Load::Mentions, Load::Detail],
                result,
            }
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
            self.set_error(format!("#{} is not assigned to you", item.id));
        }
    }

    pub(super) fn tag(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.tags().await;
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
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.board().await;
            Message::Board {
                item,
                unassign,
                result,
            }
        });
    }

    /// My PR; one that is not ready can still get auto-complete.
    pub(super) fn can_complete(&self, pr: &PullRequest) -> bool {
        self.user
            .as_ref()
            .is_some_and(|user| user.id.eq_ignore_ascii_case(&pr.created_by.id))
    }

    /// Nothing blocks the merge.
    pub(super) fn is_ready(&self, pr: &PullRequest) -> bool {
        self.data
            .pr_signals()
            .get(&pr.pull_request_id)
            .is_some_and(|signals| signals.ready)
    }

    /// Asks for the completion options first; nothing is completed until it is confirmed.
    pub(super) fn complete(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        if !self.can_complete(&pr) {
            self.set_error(format!("!{} is not your pull request", pr.pull_request_id));
            return;
        }
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.merge_strategies(&pr).await;
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

/// The command to pipe text to: the configured one, else the first native tool found.
fn clipboard_command(configured: Option<&str>) -> Option<Vec<String>> {
    if let Some(command) = configured.map(split_command).filter(|c| !c.is_empty()) {
        return Some(command);
    }
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let command: &[&str] = if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(windows) || std::env::var_os("WSL_DISTRO_NAME").is_some() {
        &["clip.exe"]
    } else if set("WAYLAND_DISPLAY") {
        &["wl-copy"]
    } else if set("DISPLAY") {
        if on_path("xclip") {
            &["xclip", "-selection", "clipboard"]
        } else {
            &["xsel", "-bi"]
        }
    } else {
        return None;
    };
    Some(command.iter().map(|part| part.to_string()).collect())
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Pipes `text` to the clipboard command; `None` when there is no command to run.
#[cfg(not(test))]
fn native_copy(configured: Option<&str>, text: &str) -> Option<anyhow::Result<()>> {
    let command = clipboard_command(configured)?;
    let explicit = configured.is_some_and(|c| !c.trim().is_empty());
    let result = run_clipboard_command(&command, text);
    match result {
        Err(_) if !explicit => None,
        result => Some(result),
    }
}

#[cfg(test)]
fn native_copy(_configured: Option<&str>, _text: &str) -> Option<anyhow::Result<()>> {
    None
}

#[cfg(not(test))]
fn run_clipboard_command(command: &[String], text: &str) -> anyhow::Result<()> {
    use std::io::Write;

    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "{} exited with {status}", command[0]);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_configured_clipboard_command_is_split_like_a_shell() {
        let command = super::clipboard_command(Some("'my copy' -x"));

        assert_eq!(command, Some(vec!["my copy".to_string(), "-x".to_string()]));
    }

    use crossterm::event::KeyCode;

    use crate::app::Popup;
    use crate::app::test_helpers::{app, assign_first_item_to, board, ctrl, me, press};

    fn with_links(count: usize) -> crate::app::App {
        let app = app();
        *app.detail_links.borrow_mut() = (0..count).map(|i| format!("https://{i}.test")).collect();
        app
    }

    #[test]
    fn link_selection_cycles_and_wraps() {
        let mut app = with_links(2);

        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.link_index, Some(0));
        press(&mut app, KeyCode::Char('l'));
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.link_index, Some(0));
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.link_index, Some(1));
    }

    #[test]
    fn previous_link_starts_at_the_last_and_no_links_selects_nothing() {
        let mut app = with_links(3);
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.link_index, Some(2));

        let mut none = with_links(0);
        press(&mut none, KeyCode::Char('l'));
        assert_eq!(none.link_index, None);
    }

    #[test]
    fn selected_link_is_the_one_acted_on() {
        let mut app = with_links(2);
        assert_eq!(app.selected_link(), None);

        app.link_index = Some(1);

        assert_eq!(app.selected_link().as_deref(), Some("https://1.test"));
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
}
