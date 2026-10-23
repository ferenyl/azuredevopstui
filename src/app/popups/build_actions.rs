use crate::api::Build;
use crate::app::message::Message;
use crate::app::{
    App, BuildAction, ConfirmAction, Detail, Popup, Selection, TextInput, TextTarget,
};

/// A change to a pipeline run, sent to Azure DevOps.
pub(in crate::app) enum BuildChange {
    Rerun,
    Queue(String),
    Cancel,
}

impl App {
    pub(in crate::app) fn open_build_actions(&mut self) {
        let Some(Detail::Build(build)) = self.current_item() else {
            return;
        };
        let actions = BuildAction::of(&build);
        let selection = Selection::new(actions.iter().map(|a| a.label().into()).collect());
        self.popup = Some(Popup::BuildActions {
            build,
            actions,
            selection,
        });
    }

    pub(in crate::app) fn confirm_build_action(
        &mut self,
        build: Box<Build>,
        actions: &[BuildAction],
        index: usize,
    ) {
        let Some(action) = actions.get(index).copied() else {
            return;
        };
        match action {
            BuildAction::Rerun => {
                let prompt = format!(
                    "Re-run {} on {}?",
                    build.definition.name,
                    short_branch(&build.source_branch)
                );
                self.popup = Some(Popup::Confirm {
                    prompt,
                    action: ConfirmAction::Rerun(build),
                });
            }
            BuildAction::Queue => {
                let text = TextInput::new(short_branch(&build.source_branch));
                self.popup = Some(Popup::Text {
                    target: TextTarget::QueueBranch(build),
                    text,
                });
            }
            BuildAction::Cancel => {
                let prompt = format!("Cancel {} {}?", build.definition.name, build.build_number);
                self.popup = Some(Popup::Confirm {
                    prompt,
                    action: ConfirmAction::CancelBuild(build),
                });
            }
        }
    }

    /// Asks before queueing; empty text keeps the branch of the run.
    pub(in crate::app) fn confirm_queue_branch(&mut self, build: Box<Build>, text: &str) {
        let branch = source_branch(text.trim(), &build.source_branch);
        let prompt = format!(
            "Queue {} on {}?",
            build.definition.name,
            short_branch(&branch)
        );
        self.popup = Some(Popup::Confirm {
            prompt,
            action: ConfirmAction::Queue { build, branch },
        });
    }

    pub(in crate::app) fn send_build_update(&mut self, build: Box<Build>, change: BuildChange) {
        let Some(api) = self.api() else {
            return;
        };
        let name = &build.definition.name;
        let label = match &change {
            BuildChange::Rerun => format!("re-running {name}"),
            BuildChange::Queue(_) => format!("queueing {name}"),
            BuildChange::Cancel => format!("cancelling {}", build.build_number),
        };
        self.spawn_action(label, async move {
            let result = match change {
                BuildChange::Rerun => api
                    .queue_build(
                        build.definition.id,
                        &build.source_branch,
                        build.source_version.as_deref(),
                    )
                    .await
                    .map(|_| ()),
                BuildChange::Queue(branch) => api
                    .queue_build(build.definition.id, &branch, None)
                    .await
                    .map(|_| ()),
                BuildChange::Cancel => api.cancel_build(build.id).await,
            };
            Message::BuildUpdated {
                id: build.id,
                result,
            }
        });
    }
}

fn short_branch(branch: &str) -> String {
    branch.strip_prefix("refs/heads/").unwrap_or(branch).into()
}

/// `text` as a full ref; `current` when it is empty.
fn source_branch(text: &str, current: &str) -> String {
    match text {
        "" => current.into(),
        text if text.starts_with("refs/") => text.into(),
        text => format!("refs/heads/{text}"),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::source_branch;
    use crate::api::Build;
    use crate::app::test_helpers::{app, press};
    use crate::app::{App, BuildAction, ConfirmAction, Detail, Panel, Popup, TextTarget};

    fn build(status: &str) -> Build {
        serde_json::from_value(serde_json::json!({
            "id": 501, "buildNumber": "n501", "status": status,
            "definition": { "id": 7, "name": "orders-ci" },
            "sourceBranch": "refs/heads/main", "sourceVersion": "abc"
        }))
        .unwrap()
    }

    fn open_menu(app: &mut App, status: &str) {
        app.focus = Panel::Detail;
        app.detail = Some(Detail::Build(Box::new(build(status))));
        press(app, KeyCode::Char('e'));
    }

    fn assert_confirm(app: &App, expected: &str) {
        let Some(Popup::Confirm { prompt, .. }) = &app.popup else {
            panic!("no confirmation");
        };
        assert_eq!(prompt, expected);
    }

    #[test]
    fn menu_offers_cancel_only_while_running_or_queued() {
        for (status, cancel) in [
            ("completed", false),
            ("inProgress", true),
            ("notStarted", true),
            ("cancelling", false),
        ] {
            assert_eq!(
                BuildAction::of(&build(status)).contains(&BuildAction::Cancel),
                cancel,
                "{status}"
            );
        }
        assert_eq!(
            BuildAction::of(&build("completed")),
            [BuildAction::Rerun, BuildAction::Queue]
        );
    }

    #[test]
    fn e_opens_the_menu_on_a_build() {
        let mut app = app();
        open_menu(&mut app, "inProgress");

        let Some(Popup::BuildActions { selection, .. }) = &app.popup else {
            panic!("menu not open");
        };
        assert_eq!(
            selection.items,
            ["Re-run (same commit)", "Queue a new run…", "Cancel run"]
        );
    }

    #[test]
    fn rerun_and_cancel_ask_first_and_n_declines() {
        let mut app = app();
        open_menu(&mut app, "inProgress");
        press(&mut app, KeyCode::Enter);
        assert_confirm(&app, "Re-run orders-ci on main?");
        assert!(matches!(
            &app.popup,
            Some(Popup::Confirm {
                action: ConfirmAction::Rerun(_),
                ..
            })
        ));
        press(&mut app, KeyCode::Char('n'));
        assert!(app.popup.is_none());

        open_menu(&mut app, "inProgress");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert_confirm(&app, "Cancel orders-ci n501?");
        press(&mut app, KeyCode::Esc);
        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }

    #[test]
    fn queue_prefills_the_branch_and_asks_before_sending() {
        let mut app = app();
        open_menu(&mut app, "completed");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            &app.popup,
            Some(Popup::Text { target: TextTarget::QueueBranch(_), text }) if text.as_str() == "main"
        ));
        for _ in 0..4 {
            press(&mut app, KeyCode::Backspace);
        }
        for c in "release".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Enter);

        assert_confirm(&app, "Queue orders-ci on release?");
        assert!(matches!(
            &app.popup,
            Some(Popup::Confirm {
                action: ConfirmAction::Queue { branch, .. },
                ..
            }) if branch == "refs/heads/release"
        ));
    }

    #[test]
    fn branch_is_normalised_to_a_ref() {
        let current = "refs/heads/main";
        assert_eq!(source_branch("", current), current);
        assert_eq!(source_branch("release", current), "refs/heads/release");
        assert_eq!(source_branch("feature/x", current), "refs/heads/feature/x");
        assert_eq!(source_branch("refs/tags/v1", current), "refs/tags/v1");
    }
}
