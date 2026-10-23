use crate::api::{PullRequest, THREAD_STATUSES};
use crate::app::message::Message;
use crate::app::{App, Detail, DetailInfo, DetailTab, Popup, Selection};

impl App {
    /// Id of the thread selected in the shown detail of `pr`, if its comments are on screen.
    pub(in crate::app) fn selected_thread_of(&self, pr: &PullRequest) -> Option<u32> {
        let (Some(Detail::PullRequest(shown)), Some(DetailInfo::PullRequest(info))) =
            (&self.detail, &self.detail_info)
        else {
            return None;
        };
        if self.detail_tab != DetailTab::Comments || shown.pull_request_id != pr.pull_request_id {
            return None;
        }
        info.ordered_threads()
            .get(self.detail_thread?)
            .map(|thread| thread.id)
    }

    /// Whether the comments of `pr` are on screen with threads to select.
    pub(in crate::app) fn has_threads_shown(&self, pr: &PullRequest) -> bool {
        matches!(
            (&self.detail, &self.detail_info),
            (Some(Detail::PullRequest(shown)), Some(DetailInfo::PullRequest(info)))
                if shown.pull_request_id == pr.pull_request_id && !info.threads.is_empty()
        ) && self.detail_tab == DetailTab::Comments
    }

    /// Moves the thread selection; the first move selects the first or last thread.
    pub(in crate::app) fn move_thread(&mut self, next: bool) {
        let Some(DetailInfo::PullRequest(info)) = &self.detail_info else {
            return;
        };
        let count = info.threads.len();
        if self.detail_tab != DetailTab::Comments || count == 0 {
            return;
        }
        self.detail_thread = Some(match (self.detail_thread, next) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(index), true) => (index + 1).min(count - 1),
            (Some(index), false) => index.saturating_sub(1),
        });
        self.scroll_to_thread = true;
    }

    pub(in crate::app) fn open_thread_status(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        let Some(thread_id) = self.selected_thread_of(&pr) else {
            return;
        };
        let Some(DetailInfo::PullRequest(info)) = &self.detail_info else {
            return;
        };
        let current = info
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .and_then(|thread| thread.status.as_deref());
        let mut selection = Selection::new(
            THREAD_STATUSES
                .iter()
                .map(|(_, label)| (*label).into())
                .collect(),
        );
        selection.selected = THREAD_STATUSES
            .iter()
            .position(|(status, _)| Some(*status) == current)
            .unwrap_or(0);
        self.popup = Some(Popup::ThreadStatus {
            pr,
            thread_id,
            selection,
        });
    }

    pub(in crate::app) fn confirm_thread_status(
        &mut self,
        pr: Box<PullRequest>,
        thread_id: u32,
        index: usize,
    ) {
        let (Some((status, _)), Some(api)) = (THREAD_STATUSES.get(index), self.api()) else {
            return;
        };
        let reload = self.reload_after_thread_change(&pr);
        let label = format!("setting a thread status on !{}", pr.pull_request_id);
        self.spawn_action(label, async move {
            let result = api.set_thread_status(&pr, thread_id, status).await;
            Message::PullRequestUpdated {
                id: pr.pull_request_id,
                reload,
                result,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;
    use serde_json::json;

    use crate::api::PullRequestDetails;
    use crate::app::test_helpers::{app, press};
    use crate::app::{App, Detail, DetailInfo, DetailTab, Panel, Popup, TextTarget};
    use crate::test_support::pull_request;

    fn thread(id: u32, status: &str, date: &str) -> serde_json::Value {
        json!({
            "id": id,
            "status": status,
            "comments": [{
                "id": id * 10,
                "author": { "id": "u", "displayName": "Anna" },
                "content": "text",
                "publishedDate": date,
                "commentType": "text"
            }]
        })
    }

    /// PR 1 on the Comments tab with threads 1 (resolved, newest), 2 (active, old), 3 (active, new).
    fn on_comments() -> App {
        let mut app = app();
        let pr = pull_request(1, "First", "me", "2026-10-01T10:00:00Z");
        app.detail = Some(Detail::PullRequest(Box::new(pr)));
        app.detail_info = Some(DetailInfo::PullRequest(PullRequestDetails {
            description: None,
            threads: serde_json::from_value(json!([
                thread(1, "fixed", "2026-10-03T10:00:00Z"),
                thread(2, "active", "2026-10-01T10:00:00Z"),
                thread(3, "active", "2026-10-02T10:00:00Z"),
            ]))
            .unwrap(),
            statuses: Vec::new(),
            policies: Vec::new(),
            work_items: Vec::new(),
        }));
        app.detail_tab = DetailTab::Comments;
        app.focus = Panel::Detail;
        app
    }

    #[test]
    fn brackets_move_the_selection_within_the_listed_order() {
        let mut app = on_comments();

        press(&mut app, KeyCode::Char(']'));
        assert_eq!(app.detail_thread, Some(0));
        for _ in 0..5 {
            press(&mut app, KeyCode::Char(']'));
        }
        assert_eq!(app.detail_thread, Some(2));
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('['));
        }

        assert_eq!(app.detail_thread, Some(0));
        assert!(app.scroll_to_thread);
    }

    #[test]
    fn prev_without_selection_starts_at_the_last_thread() {
        let mut app = on_comments();

        press(&mut app, KeyCode::Char('['));

        assert_eq!(app.detail_thread, Some(2));
    }

    #[test]
    fn brackets_do_nothing_outside_the_comments_tab() {
        let mut app = on_comments();
        app.detail_tab = DetailTab::Overview;

        press(&mut app, KeyCode::Char(']'));

        assert_eq!(app.detail_thread, None);
    }

    #[test]
    fn switching_tabs_clears_the_selection() {
        let mut app = on_comments();
        press(&mut app, KeyCode::Char(']'));

        press(&mut app, KeyCode::Tab);

        assert_eq!(app.detail_thread, None);
        assert!(!app.scroll_to_thread);
    }

    #[test]
    fn m_replies_to_the_selected_thread() {
        let mut app = on_comments();
        press(&mut app, KeyCode::Char(']'));

        press(&mut app, KeyCode::Char('m'));

        // Not resolved first, newest first: thread 3.
        assert!(matches!(
            &app.popup,
            Some(Popup::Text {
                target: TextTarget::PrThread {
                    thread_id: Some(3),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn m_without_a_selection_starts_a_new_thread() {
        let mut app = on_comments();

        press(&mut app, KeyCode::Char('m'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Text {
                target: TextTarget::PrThread {
                    thread_id: None,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn x_opens_the_status_popup_preselecting_the_current_status() {
        let mut app = on_comments();
        for _ in 0..3 {
            press(&mut app, KeyCode::Char(']'));
        }

        press(&mut app, KeyCode::Char('x'));

        let Some(Popup::ThreadStatus {
            thread_id,
            selection,
            ..
        }) = &app.popup
        else {
            panic!("status popup not open");
        };
        assert_eq!(*thread_id, 1);
        assert_eq!(selection.current(), Some("resolved"));
        assert_eq!(selection.items.len(), 4);
    }

    #[test]
    fn x_needs_a_selected_thread() {
        let mut app = on_comments();

        press(&mut app, KeyCode::Char('x'));

        assert!(app.popup.is_none());
    }
}
