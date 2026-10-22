use crate::api::PullRequest;
use crate::app::loads::Load;
use crate::app::message::Message;
use crate::app::{App, Detail, DetailInfo, Popup, TextInput, TextTarget};

impl App {
    /// Comments on the selected work item or pull request.
    pub(in crate::app) fn open_comment(&mut self) {
        let target = match self.current_item() {
            Some(Detail::WorkItem(item)) => TextTarget::WorkItemComment(item.id),
            Some(Detail::PullRequest(pr)) => {
                let thread_id = self.selected_thread_of(&pr);
                TextTarget::PrThread { pr, thread_id }
            }
            _ => return,
        };
        self.popup = Some(Popup::Text {
            target,
            text: TextInput::default(),
        });
    }

    pub(in crate::app) fn confirm_text(&mut self, target: TextTarget, text: &str) {
        match target {
            TextTarget::WorkItemComment(id) => self.confirm_comment(id, text),
            TextTarget::PrThread { pr, thread_id } => {
                self.confirm_thread_comment(pr, thread_id, text)
            }
        }
    }

    pub(in crate::app) fn confirm_comment(&mut self, id: u32, text: &str) {
        let Some(text) = markdown_comment(text) else {
            return;
        };
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.add_work_item_comment(id, &text).await;
            Message::WorkItemUpdated {
                id,
                reload: &[Load::Mentions, Load::Detail],
                result,
            }
        });
    }

    /// Replies when `thread_id` is given, otherwise starts a thread.
    fn confirm_thread_comment(&mut self, pr: Box<PullRequest>, thread_id: Option<u32>, text: &str) {
        let Some(text) = markdown_comment(text) else {
            return;
        };
        let Some(api) = self.api() else {
            return;
        };
        let parent = thread_id.map(|thread_id| self.first_comment_id(thread_id));
        let reload = self.reload_after_thread_change(&pr);
        self.spawn(async move {
            let result = match thread_id.zip(parent) {
                Some((thread_id, parent)) => api.reply(&pr, thread_id, parent, &text).await,
                None => api.create_thread(&pr, &text).await,
            };
            Message::PullRequestUpdated {
                id: pr.pull_request_id,
                reload,
                result,
            }
        });
    }

    /// What to load again after a thread changed: the detail, and the markers of the PR.
    pub(in crate::app) fn reload_after_thread_change(&self, pr: &PullRequest) -> Vec<Load> {
        let mine = self
            .user
            .as_ref()
            .is_some_and(|user| user.id.eq_ignore_ascii_case(&pr.created_by.id));
        let markers = if mine {
            Load::Signals(pr.pull_request_id)
        } else {
            Load::OtherPullRequests
        };
        vec![Load::Detail, markers]
    }

    /// Id of the first comment of a thread in the shown detail; replies hang under it.
    fn first_comment_id(&self, thread_id: u32) -> u32 {
        let Some(DetailInfo::PullRequest(info)) = &self.detail_info else {
            return 1;
        };
        info.threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .and_then(|thread| thread.comments.first())
            .map_or(1, |comment| comment.id)
    }
}

/// `None` for a blank comment.
fn markdown_comment(text: &str) -> Option<String> {
    let text = text.trim();
    // Markdown joins single newlines; trailing spaces make them line breaks.
    (!text.is_empty()).then(|| text.replace('\n', "  \n"))
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::test_helpers::{app, press};
    use crate::app::{Popup, TextInput, TextTarget};

    #[test]
    fn m_opens_comment_for_selected_work_item() {
        let mut app = app();
        app.focus = crate::app::Panel::MyWorkItems;

        press(&mut app, KeyCode::Char('m'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Text { target: TextTarget::WorkItemComment(10), text }) if text.as_str().is_empty()
        ));
    }

    #[test]
    fn blank_comment_is_dropped_on_confirm() {
        let mut app = app();
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(10),
            text: TextInput::default(),
        });
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }
}
