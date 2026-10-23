use super::{Action, App, Detail, Panel, Popup, Retry, Screen, SetupStep};

/// Every action the toolbar can show, in display order.
const TOOLBAR: [Action; 32] = [
    Action::Down,
    Action::Up,
    Action::Toggle,
    Action::FocusRight,
    Action::NextTab,
    Action::Confirm,
    Action::Cancel,
    Action::Complete,
    Action::Vote,
    Action::ChangeColumn,
    Action::Unassign,
    Action::Tag,
    Action::AssignToMe,
    Action::Comment,
    Action::NextThread,
    Action::PrevThread,
    Action::ThreadStatus,
    Action::OpenInBrowser,
    Action::Copy,
    Action::CopyLink,
    Action::NextLink,
    Action::Sort,
    Action::Filter,
    Action::Search,
    Action::PrActions,
    Action::NewWorkItem,
    Action::Zoom,
    Action::Reload,
    Action::ChangeToken,
    Action::SwitchProject,
    Action::Help,
    Action::Quit,
];

impl App {
    pub fn actions(&self) -> Vec<Action> {
        TOOLBAR
            .into_iter()
            .filter(|action| self.can(*action) && !self.keymap.label(*action).is_empty())
            .collect()
    }

    /// The toolbar text of `action`; `o` opens the selected link when there is one.
    pub fn action_label(&self, action: Action) -> &'static str {
        match action {
            Action::OpenInBrowser if self.selected_link().is_some() => "open link",
            Action::Zoom if self.zoomed => "unzoom",
            _ => action.label(),
        }
    }

    /// Whether `action` does its work in the current context, which is when the toolbar offers it.
    fn can(&self, action: Action) -> bool {
        match &self.screen {
            Screen::Setup(step) => {
                Self::can_in_setup(step, action, self.can_enter_pat(), self.can_go_back())
            }
            Screen::Main if self.show_help => action == Action::Cancel,
            Screen::Main if self.search.is_some() && self.popup.is_none() => {
                matches!(action, Action::Confirm | Action::Cancel)
            }
            Screen::Main => match &self.popup {
                Some(popup) => Self::can_in_popup(popup, action),
                None => self.can_in_main(action),
            },
        }
    }

    fn can_in_setup(
        step: &SetupStep,
        action: Action,
        can_enter_pat: bool,
        can_go_back: bool,
    ) -> bool {
        match action {
            Action::Confirm => step.is_input() || step.selection().is_some(),
            Action::Cancel => {
                step.is_input()
                    || step.selection().is_some_and(|s| s.query.is_some())
                    || (can_go_back && !matches!(step, SetupStep::Loading(_)))
            }
            Action::Search => step.selection().is_some_and(|s| s.query.is_none()),
            Action::Down | Action::Up => step.selection().is_some(),
            Action::Reload => matches!(step, SetupStep::Failed { .. }),
            Action::ChangeToken => {
                matches!(
                    step,
                    SetupStep::Failed {
                        retry: Retry::Authenticate,
                        ..
                    }
                ) && can_enter_pat
            }
            Action::Quit => {
                !step.is_input() && !step.selection().is_some_and(|s| s.query.is_some())
            }
            _ => false,
        }
    }

    fn can_in_popup(popup: &Popup, action: Action) -> bool {
        match action {
            Action::Confirm | Action::Cancel => true,
            Action::Down => !matches!(popup, Popup::Text { .. } | Popup::BuildFilter { .. }),
            Action::Toggle => matches!(
                popup,
                Popup::Types { .. } | Popup::PrFilter { .. } | Popup::Complete(_)
            ),
            _ => false,
        }
    }

    fn can_in_main(&self, action: Action) -> bool {
        let item = self.current_item();
        let is_build = matches!(item, Some(Detail::Build(_)));
        let work_item_assigned_to_me = |assigned: bool| matches!(&item, Some(Detail::WorkItem(w)) if self.is_assigned_to_me(w) == assigned);
        match action {
            Action::Down
            | Action::FocusRight
            | Action::Zoom
            | Action::Reload
            | Action::SwitchProject
            | Action::Help
            | Action::NewWorkItem
            | Action::Quit => true,
            Action::NextTab | Action::Copy => {
                matches!(item, Some(Detail::PullRequest(_) | Detail::WorkItem(_)))
            }
            Action::Complete => {
                matches!(&item, Some(Detail::PullRequest(pr)) if self.can_complete(pr))
            }
            Action::Vote => matches!(item, Some(Detail::PullRequest(_))),
            Action::PrActions => item.is_some(),
            Action::ChangeColumn => matches!(item, Some(Detail::WorkItem(_))),
            Action::Comment => matches!(item, Some(Detail::WorkItem(_) | Detail::PullRequest(_))),
            Action::NextThread | Action::PrevThread => {
                matches!(&item, Some(Detail::PullRequest(pr)) if self.has_threads_shown(pr))
            }
            Action::ThreadStatus => {
                matches!(&item, Some(Detail::PullRequest(pr)) if self.selected_thread_of(pr).is_some())
            }
            Action::Unassign => work_item_assigned_to_me(true),
            Action::Tag => matches!(item, Some(Detail::WorkItem(_))),
            Action::AssignToMe => work_item_assigned_to_me(false),
            Action::Confirm | Action::OpenInBrowser | Action::CopyLink => item.is_some(),
            Action::NextLink => !self.detail_links.borrow().is_empty(),
            Action::Sort => !is_build && self.focus != Panel::Builds,
            Action::Filter => item.is_some() || self.focus == Panel::Builds,
            Action::Search => self.focus.index().is_some(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::state::Signals;
    use crate::app::test_helpers::{app, assign_first_item_to, ctrl, me, press};
    use crate::app::{
        Action, App, Detail, Panel, Popup, Retry, Screen, Selection, SetupStep, SortKind,
        TextInput, TextTarget,
    };

    fn build() -> Detail {
        Detail::Build(Box::new(
            serde_json::from_value(serde_json::json!({
                "id": 1, "buildNumber": "1", "status": "completed", "result": "failed",
                "definition": { "id": 1, "name": "CI" }, "sourceBranch": "refs/heads/main"
            }))
            .unwrap(),
        ))
    }

    fn selection() -> Selection {
        Selection::new(vec!["a".into(), "b".into()])
    }

    /// The user who created the fixture pull requests.
    fn creator() -> crate::api::CurrentUser {
        serde_json::from_value(serde_json::json!({ "id": "me", "providerDisplayName": "Me" }))
            .unwrap()
    }

    fn step(app: &mut App, step: SetupStep) {
        app.screen = Screen::Setup(step);
    }

    fn popup(app: &mut App, popup: Popup) {
        app.popup = Some(popup);
    }

    type Setup = fn(&mut App);

    /// One fixture per toolbar context.
    fn contexts() -> Vec<(&'static str, Setup)> {
        vec![
            ("loading", |a| step(a, SetupStep::Loading("x"))),
            ("enter pat", |a| {
                step(
                    a,
                    SetupStep::EnterPat {
                        input: TextInput::default(),
                        error: None,
                        cancelable: false,
                    },
                );
            }),
            ("enter organization", |a| {
                step(a, SetupStep::EnterOrganization(TextInput::default()));
            }),
            ("select organization", |a| {
                step(a, SetupStep::SelectOrganization(selection()));
            }),
            ("failed authenticate", |a| {
                step(
                    a,
                    SetupStep::Failed {
                        error: "e".into(),
                        retry: Retry::Authenticate,
                    },
                );
            }),
            ("failed continue", |a| {
                step(
                    a,
                    SetupStep::Failed {
                        error: "e".into(),
                        retry: Retry::Continue,
                    },
                );
            }),
            ("help", |a| a.show_help = true),
            ("types popup", |a| {
                popup(
                    a,
                    Popup::Types {
                        selection: selection(),
                        checked: vec![false, false],
                    },
                );
            }),
            ("pr filter popup", |a| {
                popup(
                    a,
                    Popup::PrFilter {
                        selection: selection(),
                        checked: vec![false, false],
                    },
                );
            }),
            ("comment popup", |a| {
                popup(
                    a,
                    Popup::Text {
                        target: TextTarget::WorkItemComment(10),
                        text: TextInput::default(),
                    },
                );
            }),
            ("build filter popup", |a| {
                popup(
                    a,
                    Popup::BuildFilter {
                        text: TextInput::default(),
                    },
                );
            }),
            ("sort popup", |a| {
                popup(
                    a,
                    Popup::Sort {
                        kind: SortKind::WorkItems,
                        selection: selection(),
                    },
                );
            }),
            ("pull request", |_| {}),
            ("my pull request, not ready", |a| a.user = Some(creator())),
            ("ready pull request", |a| {
                a.user = Some(creator());
                a.data.set_pr_signals(
                    1,
                    Signals {
                        ready: true,
                        ..Signals::default()
                    },
                );
            }),
            ("my work item", |a| {
                a.user = Some(me());
                assign_first_item_to(a, "me-id");
                a.focus = Panel::MyWorkItems;
            }),
            ("others work item", |a| {
                a.user = Some(me());
                assign_first_item_to(a, "someone-else");
                a.focus = Panel::MyWorkItems;
            }),
            ("unassigned work item, no user", |a| {
                a.focus = Panel::MyWorkItems;
            }),
            ("build in detail", |a| {
                a.focus = Panel::Detail;
                a.detail = Some(build());
            }),
            ("builds focus", |a| a.focus = Panel::Builds),
            ("empty list", |a| {
                a.data.set_my_prs(Vec::new());
            }),
        ]
    }

    fn fixtures() -> impl Iterator<Item = (&'static str, App)> {
        contexts().into_iter().map(|(name, setup)| {
            let mut app = app();
            setup(&mut app);
            (name, app)
        })
    }

    #[test]
    fn toolbar_never_offers_what_apply_rejects() {
        // Skip actions with side effects outside the app (browser, clipboard, network, exit).
        let external = [
            Action::OpenInBrowser,
            Action::Copy,
            Action::CopyLink,
            Action::Reload,
            Action::Quit,
        ];
        for (name, probe) in fixtures() {
            for action in probe.actions() {
                assert!(
                    probe.can(action),
                    "{name}: {action:?} listed but not allowed"
                );
                if external.contains(&action) {
                    continue;
                }
                let (_, mut app) = fixtures().find(|(n, _)| *n == name).unwrap();
                app.apply(action);
                assert_eq!(app.error, None, "{name}: {action:?} was rejected by apply");
            }
        }
    }

    #[test]
    fn toolbar_is_the_allowed_subset_in_display_order() {
        for (name, app) in fixtures() {
            let order: Vec<_> = super::TOOLBAR
                .into_iter()
                .filter(|action| app.can(*action))
                .collect();
            assert_eq!(app.actions(), order, "{name}");
            assert!(!app.actions().is_empty(), "{name}: empty toolbar");
        }
    }

    #[test]
    fn toolbar_contexts_keep_their_actions() {
        use Action::*;
        let expected: Vec<(&str, Vec<Action>)> = vec![
            ("loading", vec![Quit]),
            ("enter pat", vec![Confirm, Cancel]),
            ("enter organization", vec![Confirm, Cancel]),
            ("select organization", vec![Down, Up, Confirm, Search, Quit]),
            ("failed authenticate", vec![Reload, ChangeToken, Quit]),
            ("failed continue", vec![Reload, Quit]),
            ("help", vec![Cancel]),
            ("types popup", vec![Down, Toggle, Confirm, Cancel]),
            ("pr filter popup", vec![Down, Toggle, Confirm, Cancel]),
            ("comment popup", vec![Confirm, Cancel]),
            ("build filter popup", vec![Confirm, Cancel]),
            ("sort popup", vec![Down, Confirm, Cancel]),
            (
                "pull request",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    Vote,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "my pull request, not ready",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    Complete,
                    Vote,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "ready pull request",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    Complete,
                    Vote,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "my work item",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    ChangeColumn,
                    Unassign,
                    Tag,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "others work item",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    ChangeColumn,
                    Tag,
                    AssignToMe,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "unassigned work item, no user",
                vec![
                    Down,
                    FocusRight,
                    NextTab,
                    Confirm,
                    ChangeColumn,
                    Tag,
                    AssignToMe,
                    Comment,
                    OpenInBrowser,
                    Copy,
                    CopyLink,
                    Sort,
                    Filter,
                    Search,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "build in detail",
                vec![
                    Down,
                    FocusRight,
                    Confirm,
                    OpenInBrowser,
                    CopyLink,
                    Filter,
                    PrActions,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "builds focus",
                vec![
                    Down,
                    FocusRight,
                    Filter,
                    Search,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
            (
                "empty list",
                vec![
                    Down,
                    FocusRight,
                    Sort,
                    Search,
                    NewWorkItem,
                    Zoom,
                    Reload,
                    SwitchProject,
                    Help,
                    Quit,
                ],
            ),
        ];
        let actual: Vec<_> = fixtures()
            .map(|(name, app)| (name, app.actions()))
            .collect();
        assert_eq!(actual.len(), expected.len());
        for ((name, actions), (expected_name, expected_actions)) in actual.iter().zip(&expected) {
            assert_eq!(name, expected_name);
            assert_eq!(actions, expected_actions, "{name}");
        }
    }

    #[test]
    fn toolbar_actions_follow_context() {
        let mut app = app();
        assert!(app.actions().contains(&Action::OpenInBrowser));
        assert!(!app.actions().contains(&Action::ChangeColumn));

        ctrl(&mut app, 'j');
        assert!(app.actions().contains(&Action::ChangeColumn));

        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.actions(), [Action::Cancel]);
    }

    #[test]
    fn set_error_keeps_every_error_in_the_history() {
        let mut app = app();

        app.set_error("first".into());
        app.set_error("second".into());

        assert_eq!(app.error.as_deref(), Some("second"));
        assert_eq!(app.activity.error_count(), 2);
    }

    #[test]
    fn errors_popup_scrolls_through_the_history_and_closes() {
        let mut app = app();
        for n in 0..3 {
            app.set_error(format!("error {n}"));
        }

        press(&mut app, KeyCode::Char('E'));
        assert!(matches!(app.popup, Some(Popup::Errors { first: 0 })));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(app.popup, Some(Popup::Errors { first: 2 })));
        press(&mut app, KeyCode::Char('k'));
        assert!(matches!(app.popup, Some(Popup::Errors { first: 1 })));
        press(&mut app, KeyCode::Esc);
        assert!(app.popup.is_none());
    }

    #[test]
    fn toolbar_offers_unassign_only_for_my_items() {
        let mut app = app();
        app.user = Some(me());
        assign_first_item_to(&mut app, "me-id");
        ctrl(&mut app, 'j');
        assert!(app.actions().contains(&Action::Unassign));

        press(&mut app, KeyCode::Char('j'));

        assert!(!app.actions().contains(&Action::Unassign));
        assert!(app.actions().contains(&Action::AssignToMe));
    }

    #[test]
    fn toolbar_offers_down_for_unassigned_items() {
        let mut app = app();
        ctrl(&mut app, 'j');

        assert!(app.actions().contains(&Action::AssignToMe));
        assert!(app.actions().contains(&Action::Down));
    }
}
