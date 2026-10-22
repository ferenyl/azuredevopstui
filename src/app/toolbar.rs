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
