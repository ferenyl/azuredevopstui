use super::{Action, App, Detail, Panel, Popup, Retry, Screen, SetupStep};

/// Every action the toolbar can show, in display order.
const TOOLBAR: [Action; 22] = [
    Action::Down,
    Action::Up,
    Action::Toggle,
    Action::FocusRight,
    Action::NextTab,
    Action::Confirm,
    Action::Cancel,
    Action::Complete,
    Action::ChangeColumn,
    Action::Unassign,
    Action::Tag,
    Action::AssignToMe,
    Action::Comment,
    Action::OpenInBrowser,
    Action::Copy,
    Action::CopyLink,
    Action::Sort,
    Action::Filter,
    Action::Reload,
    Action::ChangeToken,
    Action::Help,
    Action::Quit,
];

impl App {
    pub fn actions(&self) -> Vec<Action> {
        TOOLBAR
            .into_iter()
            .filter(|action| self.can(*action))
            .collect()
    }

    /// Whether `action` does its work in the current context, which is when the toolbar offers it.
    fn can(&self, action: Action) -> bool {
        match &self.screen {
            Screen::Setup(step) => Self::can_in_setup(step, action, self.can_enter_pat()),
            Screen::Main if self.show_help => action == Action::Cancel,
            Screen::Main => match &self.popup {
                Some(popup) => Self::can_in_popup(popup, action),
                None => self.can_in_main(action),
            },
        }
    }

    fn can_in_setup(step: &SetupStep, action: Action, can_enter_pat: bool) -> bool {
        match action {
            Action::Confirm => step.is_input() || step.selection().is_some(),
            Action::Cancel => step.is_input(),
            Action::Down | Action::Up => step.selection().is_some(),
            Action::Reload => matches!(step, SetupStep::Failed { .. }),
            Action::ChangeToken => matches!(
                step,
                SetupStep::Failed {
                    retry: Retry::Authenticate,
                    ..
                }
            ) && can_enter_pat,
            Action::Quit => !step.is_input(),
            _ => false,
        }
    }

    fn can_in_popup(popup: &Popup, action: Action) -> bool {
        match action {
            Action::Confirm | Action::Cancel => true,
            Action::Down => !matches!(popup, Popup::Comment { .. } | Popup::BuildFilter { .. }),
            Action::Toggle => matches!(popup, Popup::Types { .. } | Popup::PrFilter { .. }),
            _ => false,
        }
    }

    fn can_in_main(&self, action: Action) -> bool {
        let item = self.current_item();
        let is_build = matches!(item, Some(Detail::Build(_)));
        let work_item_assigned_to_me = |assigned: bool| {
            matches!(&item, Some(Detail::WorkItem(w)) if self.is_assigned_to_me(w) == assigned)
        };
        match action {
            Action::Down | Action::FocusRight | Action::Confirm => true,
            Action::Reload | Action::Help | Action::Quit => true,
            Action::NextTab | Action::Copy => {
                matches!(item, Some(Detail::PullRequest(_) | Detail::WorkItem(_)))
            }
            Action::Complete => {
                matches!(&item, Some(Detail::PullRequest(pr)) if self.can_complete(pr))
            }
            Action::ChangeColumn | Action::Comment => matches!(item, Some(Detail::WorkItem(_))),
            Action::Unassign | Action::Tag => work_item_assigned_to_me(true),
            Action::AssignToMe => work_item_assigned_to_me(false),
            Action::OpenInBrowser | Action::CopyLink => item.is_some(),
            Action::Sort => !is_build && self.focus != Panel::Builds,
            Action::Filter => item.is_some() || self.focus == Panel::Builds,
            _ => false,
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
