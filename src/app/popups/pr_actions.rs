use crate::api::PullRequest;
use crate::app::loads::Load;
use crate::app::message::Message;
use crate::app::{App, ConfirmAction, Detail, Popup, PrAction, Selection, TextInput, TextTarget};

/// A change to a pull request, sent to Azure DevOps.
pub(in crate::app) enum PrChange {
    Abandon,
    Reactivate,
    Draft(bool),
    Title(String),
    Description(String),
    AddReviewer(String),
    RemoveReviewer(String),
    LinkWorkItem(u32),
}

impl App {
    pub(in crate::app) fn open_pr_actions(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        let actions = PrAction::of(&pr);
        let selection = Selection::new(actions.iter().map(|a| a.label().into()).collect());
        self.popup = Some(Popup::PrActions {
            pr,
            actions,
            selection,
        });
    }

    pub(in crate::app) fn confirm_pr_action(
        &mut self,
        pr: Box<PullRequest>,
        actions: &[PrAction],
        index: usize,
    ) {
        let Some(action) = actions.get(index).copied() else {
            return;
        };
        match action {
            PrAction::Abandon => {
                let prompt = format!("Abandon !{} {}?", pr.pull_request_id, pr.title);
                self.popup = Some(Popup::Confirm {
                    prompt,
                    action: ConfirmAction::Abandon(pr),
                });
            }
            PrAction::Reactivate => self.send_pr_update(pr, PrChange::Reactivate),
            PrAction::Publish => self.send_pr_update(pr, PrChange::Draft(false)),
            PrAction::MarkAsDraft => self.send_pr_update(pr, PrChange::Draft(true)),
            PrAction::EditTitle => {
                let text = TextInput::new(pr.title.clone());
                self.popup = Some(Popup::Text {
                    target: TextTarget::PrTitle(pr),
                    text,
                });
            }
            PrAction::EditDescription => self.load_description(pr),
            PrAction::AddReviewer => {
                self.popup = Some(Popup::Text {
                    target: TextTarget::AddReviewer(pr),
                    text: TextInput::default(),
                });
            }
            PrAction::RemoveReviewer => {
                let selection = Selection::new(
                    pr.reviewers
                        .iter()
                        .map(|reviewer| reviewer.display_name.clone())
                        .collect(),
                );
                self.popup = Some(Popup::RemoveReviewer { pr, selection });
            }
            PrAction::LinkWorkItem => {
                self.popup = Some(Popup::Text {
                    target: TextTarget::LinkWorkItem(pr),
                    text: TextInput::default(),
                });
            }
        }
    }

    pub(in crate::app) fn confirm_remove_reviewer(&mut self, pr: Box<PullRequest>, index: usize) {
        let Some(reviewer) = pr.reviewers.get(index) else {
            return;
        };
        let id = reviewer.id.clone();
        self.send_pr_update(pr, PrChange::RemoveReviewer(id));
    }

    /// Fetches the full description first: the lists cut it off, and saving that would lose text.
    fn load_description(&mut self, pr: Box<PullRequest>) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.pull_request_description(&pr).await;
            Message::PrDescription { pr, result }
        });
    }

    pub(in crate::app) fn open_description_editor(
        &mut self,
        pr: Box<PullRequest>,
        description: String,
    ) {
        self.popup = Some(Popup::Text {
            target: TextTarget::PrDescription(pr),
            text: TextInput::new(description),
        });
    }

    pub(in crate::app) fn confirm_pr_title(&mut self, pr: Box<PullRequest>, text: &str) {
        let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !title.is_empty() && title != pr.title {
            self.send_pr_update(pr, PrChange::Title(title));
        }
    }

    pub(in crate::app) fn confirm_pr_description(&mut self, pr: Box<PullRequest>, text: &str) {
        self.send_pr_update(pr, PrChange::Description(text.trim_end().to_string()));
    }

    pub(in crate::app) fn confirm_add_reviewer(&mut self, pr: Box<PullRequest>, text: &str) {
        let name = text.trim();
        if !name.is_empty() {
            self.send_pr_update(pr, PrChange::AddReviewer(name.to_string()));
        }
    }

    pub(in crate::app) fn confirm_link_work_item(&mut self, pr: Box<PullRequest>, text: &str) {
        match text.trim().trim_start_matches('#').parse() {
            Ok(id) => self.send_pr_update(pr, PrChange::LinkWorkItem(id)),
            Err(_) => self.set_error(format!("not a work item id: {}", text.trim())),
        }
    }

    pub(in crate::app) fn send_pr_update(&mut self, pr: Box<PullRequest>, change: PrChange) {
        let Some(api) = self.api() else {
            return;
        };
        let reload = vec![Load::MyPullRequests, Load::OtherPullRequests, Load::Detail];
        let number = pr.pull_request_id;
        let label = match &change {
            PrChange::Abandon => format!("abandoning !{number}"),
            PrChange::Reactivate => format!("reactivating !{number}"),
            PrChange::Draft(true) => format!("marking !{number} as draft"),
            PrChange::Draft(false) => format!("publishing !{number}"),
            PrChange::Title(_) => format!("renaming !{number}"),
            PrChange::Description(_) => format!("editing the description of !{number}"),
            PrChange::AddReviewer(_) => format!("adding a reviewer to !{number}"),
            PrChange::RemoveReviewer(_) => format!("removing a reviewer from !{number}"),
            PrChange::LinkWorkItem(id) => format!("linking #{id} to !{number}"),
        };
        self.spawn_action(label, async move {
            let result = match change {
                PrChange::Abandon => api.abandon(&pr).await,
                PrChange::Reactivate => api.reactivate(&pr).await,
                PrChange::Draft(is_draft) => api.set_draft(&pr, is_draft).await,
                PrChange::Title(title) => api.set_title(&pr, &title).await,
                PrChange::Description(text) => api.set_description(&pr, &text).await,
                PrChange::AddReviewer(name) => api.add_reviewer(&pr, &name).await,
                PrChange::RemoveReviewer(id) => api.remove_reviewer(&pr, &id).await,
                PrChange::LinkWorkItem(id) => api.link_work_item(&pr, id).await,
            };
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

    use crate::app::test_helpers::{app, open_pull_request, press};
    use crate::app::{ConfirmAction, Popup, PrAction, TextTarget};
    use crate::test_support::pull_request;

    fn labels(app: &crate::app::App) -> Vec<String> {
        let Some(Popup::PrActions { selection, .. }) = &app.popup else {
            panic!("menu not open");
        };
        selection.items.clone()
    }

    #[test]
    fn menu_depends_on_the_state_of_the_pull_request() {
        let mut pr = pull_request(1, "PR", "me", "2026-10-01T10:00:00Z");
        pr.status = "active".into();
        assert_eq!(
            PrAction::of(&pr),
            [
                PrAction::Abandon,
                PrAction::MarkAsDraft,
                PrAction::EditTitle,
                PrAction::EditDescription,
                PrAction::AddReviewer,
                PrAction::RemoveReviewer,
                PrAction::LinkWorkItem,
            ]
        );

        pr.is_draft = true;
        pr.reviewers.clear();
        assert_eq!(PrAction::of(&pr)[1], PrAction::Publish);
        assert!(!PrAction::of(&pr).contains(&PrAction::RemoveReviewer));

        pr.status = "abandoned".into();
        let actions = PrAction::of(&pr);
        assert_eq!(actions[0], PrAction::Reactivate);
        assert!(!actions.contains(&PrAction::Abandon));
        assert!(!actions.contains(&PrAction::Publish));
    }

    #[test]
    fn e_opens_the_menu() {
        let mut app = app();
        open_pull_request(&mut app);

        press(&mut app, KeyCode::Char('e'));

        assert_eq!(labels(&app)[0], "Abandon");
    }

    #[test]
    fn abandoning_asks_for_confirmation_first() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));

        press(&mut app, KeyCode::Enter);

        assert!(matches!(
            &app.popup,
            Some(Popup::Confirm {
                action: ConfirmAction::Abandon(pr),
                ..
            }) if pr.pull_request_id == 1
        ));
    }

    #[test]
    fn declining_the_confirmation_closes_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('n'));

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }

    #[test]
    fn edit_title_prefills_the_title() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));

        press(&mut app, KeyCode::Enter);

        assert!(matches!(
            &app.popup,
            Some(Popup::Text { target: TextTarget::PrTitle(_), text }) if text.as_str() == "First"
        ));
    }

    #[test]
    fn remove_reviewer_lists_the_reviewers() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('j'));
        }

        press(&mut app, KeyCode::Enter);

        let Some(Popup::RemoveReviewer { pr, selection }) = &app.popup else {
            panic!("reviewer list not open");
        };
        assert_eq!(selection.items.len(), pr.reviewers.len());
    }

    #[test]
    fn a_work_item_id_must_be_a_number() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('j'));
        }
        press(&mut app, KeyCode::Enter);
        for c in "abc".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Enter);

        assert_eq!(app.error.as_deref(), Some("not a work item id: abc"));
    }

    #[test]
    fn blank_title_and_reviewer_are_dropped() {
        let mut app = app();
        let pr = || Box::new(pull_request(1, "First", "me", "2026-10-01T10:00:00Z"));

        app.confirm_pr_title(pr(), "  First ");
        app.confirm_pr_title(pr(), "   ");
        app.confirm_add_reviewer(pr(), "  ");

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }
}
