use std::sync::Arc;

use crate::api::{CompletionOptions, PullRequest};
use crate::app::loads::Load;
use crate::app::message::Message;
use crate::app::{App, CompletePicker, CompleteRow, ConfirmAction, Popup, Selection, TextInput};
use crate::config::MergeStrategy;

const BYPASS_REASON: &str = "Bypassed from azuredevopstui";

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
        let ready = self.is_ready(&pr);
        let last = self.config.as_ref().and_then(|c| c.merge_strategy);
        let defaults = self
            .config
            .as_ref()
            .map(|c| c.completion)
            .unwrap_or_default();
        let strategy = strategies
            .iter()
            .position(|s| Some(*s) == last)
            .unwrap_or(0);
        let mut picker = CompletePicker {
            message: TextInput::new(format!("Merged PR {}: {}", pr.pull_request_id, pr.title)),
            auto_complete: !ready,
            pr,
            strategies,
            strategy,
            selection: Selection::new(Vec::new()),
            ready,
            delete_source_branch: defaults.delete_source_branch,
            transition_work_items: defaults.transition_work_items,
            bypass_policy: false,
        };
        picker.selection = Selection::new(picker.items());
        picker.selection.selected = strategy;
        self.popup = Some(Popup::Complete(picker));
    }

    pub(in crate::app) fn confirm_complete(&mut self, mut picker: CompletePicker) {
        if picker.row() == CompleteRow::CancelAutoComplete {
            return self.cancel_auto_complete(picker.pr);
        }
        if let CompleteRow::Strategy(index) = picker.row() {
            picker.strategy = index;
        }
        if !picker.ready && !picker.auto_complete && !picker.bypass_policy {
            self.error = Some(format!(
                "!{} is not ready to complete: set auto-complete or bypass branch policies",
                picker.pr.pull_request_id
            ));
            self.popup = Some(Popup::Complete(picker));
            return;
        }
        let Some(strategy) = picker.strategies.get(picker.strategy).copied() else {
            return;
        };
        let message = picker.message.as_str().trim();
        let options = CompletionOptions {
            merge_strategy: strategy,
            delete_source_branch: picker.delete_source_branch,
            transition_work_items: picker.transition_work_items,
            merge_commit_message: (!message.is_empty()).then(|| message.to_string()),
            bypass_policy: picker.bypass_policy,
            bypass_reason: picker.bypass_policy.then(|| BYPASS_REASON.to_string()),
        };
        let (pr, auto_complete) = (picker.pr, picker.auto_complete);
        if options.bypass_policy {
            let what = if auto_complete {
                "Set auto-complete that bypasses"
            } else {
                "Complete"
            };
            let prompt = format!("{what} !{} and bypass branch policies?", pr.pull_request_id);
            self.popup = Some(Popup::Confirm {
                prompt,
                action: ConfirmAction::Complete {
                    pr,
                    options,
                    auto_complete,
                },
            });
        } else {
            self.send_completion(pr, options, auto_complete);
        }
    }

    pub(super) fn send_completion(
        &mut self,
        pr: Box<PullRequest>,
        options: CompletionOptions,
        auto_complete: bool,
    ) {
        let saved = self.config.as_mut().map(|config| {
            let config = Arc::make_mut(config);
            config.merge_strategy = Some(options.merge_strategy);
            config.completion.delete_source_branch = options.delete_source_branch;
            config.completion.transition_work_items = options.transition_work_items;
            config.save()
        });
        if let Some(Err(err)) = saved {
            self.report_error(err);
        }
        let (Some(api), Some(user)) = (self.api(), self.user.as_ref()) else {
            return;
        };
        let user_id = user.id.clone();
        self.spawn(async move {
            let (result, reload) = if auto_complete {
                (
                    api.set_auto_complete(&pr, &user_id, &options).await,
                    vec![Load::MyPullRequests, Load::Detail],
                )
            } else {
                (
                    api.complete_pull_request(&pr, &options).await,
                    vec![Load::MyPullRequests, Load::OtherPullRequests, Load::Detail],
                )
            };
            Message::PullRequestUpdated {
                id: pr.pull_request_id,
                reload,
                result,
            }
        });
    }

    fn cancel_auto_complete(&mut self, pr: Box<PullRequest>) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move {
            let result = api.cancel_auto_complete(&pr).await;
            Message::PullRequestUpdated {
                id: pr.pull_request_id,
                reload: vec![Load::MyPullRequests, Load::Detail],
                result,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::KeyCode;

    use crate::app::state::Signals;
    use crate::app::test_helpers::{app, press};
    use crate::app::{CompletePicker, CompleteRow, ConfirmAction, Popup};
    use crate::config::{CompletionDefaults, MergeStrategy};
    use crate::test_support::pull_request;

    #[test]
    fn picker_preselects_last_used_strategy() {
        let mut app = app();
        Arc::make_mut(app.config.as_mut().unwrap()).merge_strategy = Some(MergeStrategy::Rebase);
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

    fn open(app: &mut crate::app::App, ready: bool) {
        let pr = pull_request(1, "First", "me", "2026-10-03T10:00:00Z");
        if ready {
            app.data.set_pr_signals(
                1,
                Signals {
                    ready: true,
                    ..Default::default()
                },
            );
        }
        app.open_complete_picker(
            Box::new(pr),
            vec![MergeStrategy::Squash, MergeStrategy::Rebase],
        );
    }

    fn picker(app: &crate::app::App) -> &CompletePicker {
        let Some(Popup::Complete(picker)) = &app.popup else {
            panic!("complete picker not open");
        };
        picker
    }

    fn go_to(app: &mut crate::app::App, row: CompleteRow) {
        while picker(app).row() != row {
            press(app, KeyCode::Down);
        }
    }

    #[test]
    fn options_are_preselected_from_the_config() {
        let mut app = app();
        Arc::make_mut(app.config.as_mut().unwrap()).completion = CompletionDefaults {
            delete_source_branch: true,
            transition_work_items: false,
        };

        open(&mut app, true);

        let picker = picker(&app);
        assert!(picker.delete_source_branch);
        assert!(!picker.transition_work_items);
        assert!(!picker.auto_complete);
        assert_eq!(picker.message.as_str(), "Merged PR 1: First");
    }

    #[test]
    fn a_pull_request_that_is_not_ready_starts_with_auto_complete() {
        let mut app = app();

        open(&mut app, false);

        assert!(picker(&app).auto_complete);
    }

    #[test]
    fn space_toggles_the_option_and_picks_the_strategy() {
        let mut app = app();
        open(&mut app, true);

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        go_to(&mut app, CompleteRow::TransitionWorkItems);
        press(&mut app, KeyCode::Char(' '));

        let picker = picker(&app);
        assert_eq!(picker.strategy, 1);
        assert!(picker.transition_work_items);
        assert!(picker.items()[3].starts_with("[x]"));
    }

    #[test]
    fn typing_on_the_message_row_edits_the_message() {
        let mut app = app();
        open(&mut app, true);
        go_to(&mut app, CompleteRow::Message);

        press(&mut app, KeyCode::Char('!'));
        press(&mut app, KeyCode::Char('q'));

        assert_eq!(picker(&app).message.as_str(), "Merged PR 1: First!q");
        assert!(!app.should_quit);
    }

    #[test]
    fn confirming_saves_the_options() {
        let mut app = app();
        open(&mut app, true);
        go_to(&mut app, CompleteRow::DeleteSourceBranch);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        let config = app.config.as_ref().unwrap();
        assert!(config.completion.delete_source_branch);
        assert!(!config.completion.transition_work_items);
        assert_eq!(config.merge_strategy, Some(MergeStrategy::Squash));
    }

    #[test]
    fn bypassing_policies_asks_for_confirmation_first() {
        let mut app = app();
        open(&mut app, true);
        go_to(&mut app, CompleteRow::BypassPolicy);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        let Some(Popup::Confirm {
            prompt,
            action: ConfirmAction::Complete { options, .. },
        }) = &app.popup
        else {
            panic!("confirm popup not open");
        };
        assert!(prompt.contains("bypass"));
        assert!(options.bypass_policy);
        assert!(options.bypass_reason.is_some());
    }

    #[test]
    fn declining_the_bypass_closes_the_popup() {
        let mut app = app();
        open(&mut app, true);
        go_to(&mut app, CompleteRow::BypassPolicy);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('n'));

        assert!(app.popup.is_none());
    }

    #[test]
    fn not_ready_without_auto_complete_or_bypass_keeps_the_popup_open() {
        let mut app = app();
        open(&mut app, false);
        go_to(&mut app, CompleteRow::AutoComplete);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        assert!(matches!(app.popup, Some(Popup::Complete(_))));
        assert!(app.error.as_deref().unwrap().contains("not ready"));
    }

    #[test]
    fn cancelling_auto_complete_is_offered_only_when_it_is_set() {
        let mut app = app();
        open(&mut app, false);
        assert!(
            !picker(&app)
                .items()
                .contains(&"Cancel auto-complete".into())
        );
        app.popup = None;
        let mut pr = pull_request(1, "First", "me", "2026-10-03T10:00:00Z");
        pr.auto_complete_set_by = Some(
            serde_json::from_value(serde_json::json!({ "id": "me-id", "displayName": "Mona" }))
                .unwrap(),
        );

        app.open_complete_picker(Box::new(pr), vec![MergeStrategy::Squash]);

        let picker = picker(&app);
        assert!(picker.auto_complete);
        assert_eq!(picker.items().last().unwrap(), "Cancel auto-complete");
    }

    #[test]
    fn declining_the_bypass_saves_nothing() {
        let mut app = app();
        open(&mut app, true);
        go_to(&mut app, CompleteRow::DeleteSourceBranch);
        press(&mut app, KeyCode::Char(' '));
        go_to(&mut app, CompleteRow::BypassPolicy);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('n'));

        let config = app.config.as_ref().unwrap();
        assert!(!config.completion.delete_source_branch);
        assert_eq!(config.merge_strategy, None);
    }

    #[test]
    fn a_ready_pull_request_with_auto_complete_set_preselects_completing_now() {
        let mut app = app();
        let mut pr = pull_request(1, "First", "me", "2026-10-03T10:00:00Z");
        pr.auto_complete_set_by = Some(
            serde_json::from_value(serde_json::json!({ "id": "me-id", "displayName": "Mona" }))
                .unwrap(),
        );
        app.data.set_pr_signals(
            1,
            Signals {
                ready: true,
                ..Default::default()
            },
        );

        app.open_complete_picker(Box::new(pr), vec![MergeStrategy::Squash]);

        assert!(!picker(&app).auto_complete);
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
