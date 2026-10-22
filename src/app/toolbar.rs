use super::{Action, App, Detail, Panel, Popup, Retry, Screen, SetupStep};

impl App {
    pub fn actions(&self) -> &'static [Action] {
        match &self.screen {
            Screen::Setup(step) if step.is_input() => &[Action::Confirm, Action::Cancel],
            Screen::Setup(SetupStep::Failed {
                retry: Retry::Authenticate,
                ..
            }) if self.can_enter_pat() => &[Action::Reload, Action::ChangeToken, Action::Quit],
            Screen::Setup(SetupStep::Failed { .. }) => &[Action::Reload, Action::Quit],
            Screen::Setup(step) if step.selection().is_some() => {
                &[Action::Down, Action::Up, Action::Confirm, Action::Quit]
            }
            Screen::Setup(_) => &[Action::Quit],
            Screen::Main if self.show_help => &[Action::Cancel],
            Screen::Main
                if matches!(
                    self.popup,
                    Some(Popup::Types { .. } | Popup::PrFilter { .. })
                ) =>
            {
                &[
                    Action::Down,
                    Action::Toggle,
                    Action::Confirm,
                    Action::Cancel,
                ]
            }
            Screen::Main
                if matches!(
                    self.popup,
                    Some(Popup::Comment { .. } | Popup::BuildFilter { .. })
                ) =>
            {
                &[Action::Confirm, Action::Cancel]
            }
            Screen::Main if self.popup.is_some() => {
                &[Action::Down, Action::Confirm, Action::Cancel]
            }
            Screen::Main => match self.current_item() {
                Some(Detail::PullRequest(pr)) if self.can_complete(&pr) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::Complete,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::PullRequest(_)) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::WorkItem(item)) if self.is_assigned_to_me(&item) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::ChangeColumn,
                    Action::Unassign,
                    Action::Tag,
                    Action::Comment,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::Build(_)) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::Confirm,
                    Action::OpenInBrowser,
                    Action::CopyLink,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                Some(Detail::WorkItem(_)) => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::NextTab,
                    Action::Confirm,
                    Action::ChangeColumn,
                    Action::AssignToMe,
                    Action::Comment,
                    Action::OpenInBrowser,
                    Action::Copy,
                    Action::CopyLink,
                    Action::Sort,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                None if self.focus == Panel::Builds => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::Filter,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
                None => &[
                    Action::Down,
                    Action::FocusRight,
                    Action::Sort,
                    Action::Reload,
                    Action::Help,
                    Action::Quit,
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::Action;
    use crate::app::test_helpers::{app, assign_first_item_to, ctrl, me, press};

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
