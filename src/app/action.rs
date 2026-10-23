#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Cancel,
    Up,
    Down,
    Confirm,
    Reload,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    OpenInBrowser,
    Copy,
    CopyLink,
    AssignToMe,
    Unassign,
    Tag,
    Comment,
    ChangeColumn,
    Complete,
    ChangeToken,
    Sort,
    Filter,
    Sprint,
    Toggle,
    NextTab,
    PrevTab,
    Help,
    NextLink,
    PrevLink,
    Vote,
    NextThread,
    PrevThread,
    ThreadStatus,
    PrActions,
    SwitchProject,
    Zoom,
    Top,
    Bottom,
    PageDown,
    PageUp,
    HalfPageDown,
    HalfPageUp,
    /// Focuses the left box with this number, starting at 1.
    JumpToPanel(u8),
    Search,
    Errors,
    NewWorkItem,
    SprintBoard,
}

/// Actions listed in the help popup, by where they apply.
pub const HELP: [(&str, &[Action]); 4] = [
    (
        "General",
        &[
            Action::Down,
            Action::Top,
            Action::PageDown,
            Action::HalfPageDown,
            Action::JumpToPanel(1),
            Action::FocusRight,
            Action::Zoom,
            Action::NextTab,
            Action::Confirm,
            Action::OpenInBrowser,
            Action::Copy,
            Action::CopyLink,
            Action::NextLink,
            Action::PrevLink,
            Action::Sprint,
            Action::SprintBoard,
            Action::Reload,
            Action::Errors,
            Action::ChangeToken,
            Action::SwitchProject,
            Action::Help,
            Action::Cancel,
            Action::Quit,
        ],
    ),
    ("Lists", &[Action::Sort, Action::Filter, Action::Search]),
    (
        "Work items",
        &[
            Action::ChangeColumn,
            Action::AssignToMe,
            Action::Unassign,
            Action::Tag,
            Action::Comment,
            Action::PrActions,
            Action::NewWorkItem,
        ],
    ),
    (
        "Pull requests",
        &[
            Action::Complete,
            Action::Vote,
            Action::Comment,
            Action::NextThread,
            Action::PrevThread,
            Action::ThreadStatus,
            Action::PrActions,
        ],
    ),
];

impl Action {
    /// Every action that can be bound in the config's `keys`; `Toggle` (`space`) is fixed.
    pub const ALL: [Self; 51] = [
        Self::Quit,
        Self::Cancel,
        Self::Up,
        Self::Down,
        Self::Confirm,
        Self::Reload,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::OpenInBrowser,
        Self::Copy,
        Self::CopyLink,
        Self::AssignToMe,
        Self::Unassign,
        Self::Tag,
        Self::Comment,
        Self::ChangeColumn,
        Self::Complete,
        Self::ChangeToken,
        Self::Sort,
        Self::Filter,
        Self::Sprint,
        Self::NextTab,
        Self::PrevTab,
        Self::Help,
        Self::NextLink,
        Self::PrevLink,
        Self::Vote,
        Self::NextThread,
        Self::PrevThread,
        Self::ThreadStatus,
        Self::PrActions,
        Self::SwitchProject,
        Self::Zoom,
        Self::Top,
        Self::Bottom,
        Self::PageDown,
        Self::PageUp,
        Self::HalfPageDown,
        Self::HalfPageUp,
        Self::JumpToPanel(1),
        Self::JumpToPanel(2),
        Self::JumpToPanel(3),
        Self::JumpToPanel(4),
        Self::JumpToPanel(5),
        Self::JumpToPanel(6),
        Self::Search,
        Self::Errors,
        Self::NewWorkItem,
        Self::SprintBoard,
    ];

    /// Name in the config's `keys`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Cancel => "cancel",
            Self::Up => "up",
            Self::Down => "down",
            Self::Confirm => "confirm",
            Self::Reload => "reload",
            Self::FocusLeft => "focus_left",
            Self::FocusRight => "focus_right",
            Self::FocusUp => "focus_up",
            Self::FocusDown => "focus_down",
            Self::OpenInBrowser => "open_in_browser",
            Self::Copy => "copy",
            Self::CopyLink => "copy_link",
            Self::AssignToMe => "assign_to_me",
            Self::Unassign => "unassign",
            Self::Tag => "tag",
            Self::Comment => "comment",
            Self::ChangeColumn => "change_column",
            Self::Complete => "complete",
            Self::ChangeToken => "change_token",
            Self::Sort => "sort",
            Self::Filter => "filter",
            Self::Sprint => "sprint",
            Self::Toggle => "toggle",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::Help => "help",
            Self::NextLink => "next_link",
            Self::PrevLink => "prev_link",
            Self::Vote => "vote",
            Self::NextThread => "next_thread",
            Self::PrevThread => "prev_thread",
            Self::ThreadStatus => "thread_status",
            Self::PrActions => "pr_actions",
            Self::SwitchProject => "switch_project",
            Self::Zoom => "zoom",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::PageDown => "page_down",
            Self::PageUp => "page_up",
            Self::HalfPageDown => "half_page_down",
            Self::HalfPageUp => "half_page_up",
            Self::JumpToPanel(1) => "panel_1",
            Self::JumpToPanel(2) => "panel_2",
            Self::JumpToPanel(3) => "panel_3",
            Self::JumpToPanel(4) => "panel_4",
            Self::JumpToPanel(5) => "panel_5",
            Self::JumpToPanel(_) => "panel_6",
            Self::Search => "search",
            Self::Errors => "errors",
            Self::NewWorkItem => "new_work_item",
            Self::SprintBoard => "board",
        }
    }

    /// Actions shown as one entry in the toolbar and help, with the first key of each.
    pub fn group(self) -> Option<&'static [Self]> {
        match self {
            Self::Down => Some(&[Self::Down, Self::Up]),
            Self::FocusRight => Some(&[
                Self::FocusLeft,
                Self::FocusDown,
                Self::FocusUp,
                Self::FocusRight,
            ]),
            Self::Top => Some(&[Self::Top, Self::Bottom]),
            Self::PageDown => Some(&[Self::PageDown, Self::PageUp]),
            Self::HalfPageDown => Some(&[Self::HalfPageDown, Self::HalfPageUp]),
            Self::JumpToPanel(_) => Some(&[
                Self::JumpToPanel(1),
                Self::JumpToPanel(2),
                Self::JumpToPanel(3),
                Self::JumpToPanel(4),
                Self::JumpToPanel(5),
                Self::JumpToPanel(6),
            ]),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Cancel => "close/cancel",
            Self::Up => "up",
            Self::Down => "move/scroll",
            Self::Confirm => "select",
            Self::Reload => "reload",
            Self::FocusLeft => "left",
            Self::FocusRight => "change box/column",
            Self::FocusUp => "box up",
            Self::FocusDown => "box down",
            Self::OpenInBrowser => "open in browser",
            Self::Copy => "copy number",
            Self::CopyLink => "copy link",
            Self::AssignToMe => "assign to me",
            Self::Unassign => "unassign",
            Self::Tag => "edit tags",
            Self::Comment => "comment",
            Self::ChangeColumn => "change status",
            Self::Complete => "complete PR",
            Self::ChangeToken => "set PAT",
            Self::Sort => "sort",
            Self::Filter => "filter",
            Self::Sprint => "change sprint",
            Self::Toggle => "toggle",
            Self::NextTab => "next tab",
            Self::PrevTab => "previous tab",
            Self::Help => "help",
            Self::NextLink => "select link",
            Self::PrevLink => "previous link",
            Self::Vote => "vote",
            Self::NextThread => "next thread",
            Self::PrevThread => "previous thread",
            Self::ThreadStatus => "thread status",
            Self::PrActions => "actions",
            Self::SwitchProject => "change project",
            Self::Zoom => "zoom",
            Self::Top => "top/bottom",
            Self::Bottom => "bottom",
            Self::PageDown => "page down/up",
            Self::PageUp => "page up",
            Self::HalfPageDown => "half page down/up",
            Self::HalfPageUp => "half page up",
            Self::JumpToPanel(_) => "go to box",
            Self::Search => "search",
            Self::Errors => "errors",
            Self::NewWorkItem => "new work item",
            Self::SprintBoard => "sprint board",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Keymap;

    #[test]
    fn names_are_unique_snake_case() {
        let mut names: Vec<_> = Action::ALL.iter().map(|a| a.name()).collect();
        assert!(names.iter().all(|n| {
            n.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Action::ALL.len());
    }

    #[test]
    fn help_entries_have_labels() {
        let keymap = Keymap::default();
        for action in HELP.iter().flat_map(|(_, actions)| actions.iter()) {
            assert!(!keymap.label(*action).is_empty(), "{action:?}");
            assert!(!action.label().is_empty(), "{action:?}");
        }
    }
}
