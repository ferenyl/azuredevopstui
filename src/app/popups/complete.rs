use crate::api::PullRequest;
use crate::app::message::Message;
use crate::app::{App, CompletePicker, Popup, Selection};
use crate::config::MergeStrategy;

impl App {
    pub(in crate::app) fn open_complete_picker(
        &mut self,
        pr: Box<PullRequest>,
        strategies: Vec<MergeStrategy>,
    ) {
        if strategies.is_empty() {
            self.error = Some(format!(
                "no merge strategy is allowed for !{}",
                pr.pull_request_id
            ));
            return;
        }
        let labels = strategies.iter().map(|s| s.label().into()).collect();
        let mut selection = Selection::new(labels);
        let last = self.config.as_ref().and_then(|c| c.merge_strategy);
        selection.selected = strategies
            .iter()
            .position(|s| Some(*s) == last)
            .unwrap_or(0);
        self.popup = Some(Popup::Complete(CompletePicker {
            pr,
            strategies,
            selection,
        }));
    }

    pub(in crate::app) fn confirm_complete(&mut self, picker: CompletePicker) {
        let Some(strategy) = picker.strategies.get(picker.selection.selected).copied() else {
            return;
        };
        let (Some(client), Some(config)) = (self.client.clone(), &mut self.config) else {
            return;
        };
        config.merge_strategy = Some(strategy);
        let saved = config.save();
        let (config, pr) = (config.clone(), picker.pr);
        self.spawn(async move {
            let result = client
                .complete_pull_request(&config.organization, &config.project, &pr, strategy)
                .await;
            Message::PullRequestCompleted {
                id: pr.pull_request_id,
                result,
            }
        });
        if let Err(err) = saved {
            self.report_error(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::Popup;
    use crate::app::test_helpers::{app, press};
    use crate::config::MergeStrategy;
    use crate::test_support::pull_request;

    #[test]
    fn picker_preselects_last_used_strategy() {
        let mut app = app();
        app.config.as_mut().unwrap().merge_strategy = Some(MergeStrategy::Rebase);
        let pr = Box::new(pull_request(1, "First", "me", "2026-10-03T10:00:00Z"));

        app.open_complete_picker(pr, vec![MergeStrategy::Squash, MergeStrategy::Rebase]);

        let Some(Popup::Complete(picker)) = &app.popup else {
            panic!("complete picker not open");
        };
        assert_eq!(picker.selection.selected, 1);
    }

    #[test]
    fn no_allowed_strategy_shows_error() {
        let mut app = app();
        let pr = Box::new(pull_request(1, "First", "me", "2026-10-03T10:00:00Z"));

        app.open_complete_picker(pr, Vec::new());

        assert!(app.popup.is_none());
        assert_eq!(
            app.error.as_deref(),
            Some("no merge strategy is allowed for !1")
        );
    }

    #[test]
    fn esc_cancels_complete() {
        let mut app = app();
        let pr = Box::new(pull_request(1, "First", "me", "2026-10-03T10:00:00Z"));
        app.open_complete_picker(pr, vec![MergeStrategy::Squash]);

        press(&mut app, KeyCode::Esc);

        assert!(app.popup.is_none());
    }
}
