use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
            Action::Reload,
            Action::ChangeToken,
            Action::SwitchProject,
            Action::Help,
            Action::Cancel,
            Action::Quit,
        ],
    ),
    ("Lists", &[Action::Sort, Action::Filter]),
    (
        "Work items",
        &[
            Action::ChangeColumn,
            Action::AssignToMe,
            Action::Unassign,
            Action::Tag,
            Action::Comment,
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
    pub fn from_key(key: KeyEvent) -> Option<Self> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Self::Quit),
            (KeyCode::Char('h') | KeyCode::Left, KeyModifiers::CONTROL) => Some(Self::FocusLeft),
            (KeyCode::Char('l') | KeyCode::Right, KeyModifiers::CONTROL) => Some(Self::FocusRight),
            (KeyCode::Char('k') | KeyCode::Up, KeyModifiers::CONTROL) => Some(Self::FocusUp),
            (KeyCode::Char('j') | KeyCode::Down, KeyModifiers::CONTROL) => Some(Self::FocusDown),
            (KeyCode::Char('q'), _) => Some(Self::Quit),
            (KeyCode::Esc, _) => Some(Self::Cancel),
            (KeyCode::Char('j') | KeyCode::Down, _) => Some(Self::Down),
            (KeyCode::Char('k') | KeyCode::Up, _) => Some(Self::Up),
            (KeyCode::Enter, _) => Some(Self::Confirm),
            (KeyCode::Char('r'), _) => Some(Self::Reload),
            (KeyCode::Char('o'), _) => Some(Self::OpenInBrowser),
            (KeyCode::Char('y'), _) => Some(Self::Copy),
            (KeyCode::Char('Y'), _) => Some(Self::CopyLink),
            (KeyCode::Char('a'), _) => Some(Self::AssignToMe),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Self::HalfPageDown),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => Some(Self::HalfPageUp),
            (KeyCode::Char('u'), _) => Some(Self::Unassign),
            (KeyCode::Char('g'), _) => Some(Self::Top),
            (KeyCode::Char('G'), _) => Some(Self::Bottom),
            (KeyCode::PageDown, _) => Some(Self::PageDown),
            (KeyCode::PageUp, _) => Some(Self::PageUp),
            (KeyCode::Char(c @ '1'..='6'), _) => Some(Self::JumpToPanel(c as u8 - b'0')),
            (KeyCode::Char('s'), _) => Some(Self::ChangeColumn),
            (KeyCode::Char('c'), _) => Some(Self::Complete),
            (KeyCode::Char('S'), _) => Some(Self::Sort),
            (KeyCode::Char('t'), _) => Some(Self::Tag),
            (KeyCode::Char('m'), _) => Some(Self::Comment),
            (KeyCode::Char('f'), _) => Some(Self::Filter),
            (KeyCode::Char('i'), _) => Some(Self::Sprint),
            (KeyCode::Char(' '), _) => Some(Self::Toggle),
            (KeyCode::Tab, _) => Some(Self::NextTab),
            (KeyCode::BackTab, _) => Some(Self::PrevTab),
            (KeyCode::Char('T'), _) => Some(Self::ChangeToken),
            (KeyCode::Char('l'), _) => Some(Self::NextLink),
            (KeyCode::Char('L'), _) => Some(Self::PrevLink),
            (KeyCode::Char('?'), _) => Some(Self::Help),
            (KeyCode::Char('v'), _) => Some(Self::Vote),
            (KeyCode::Char(']'), _) => Some(Self::NextThread),
            (KeyCode::Char('['), _) => Some(Self::PrevThread),
            (KeyCode::Char('x'), _) => Some(Self::ThreadStatus),
            (KeyCode::Char('e'), _) => Some(Self::PrActions),
            (KeyCode::Char('P'), _) => Some(Self::SwitchProject),
            (KeyCode::Char('z'), _) => Some(Self::Zoom),
            _ => None,
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            Self::Quit => "q",
            Self::Cancel => "esc",
            Self::Up => "k/↑",
            Self::Down => "j/k/↑/↓",
            Self::Confirm => "enter",
            Self::Reload => "r",
            Self::FocusLeft => "ctrl+h/←",
            Self::FocusRight => "ctrl+hjkl",
            Self::FocusUp => "ctrl+k/↑",
            Self::FocusDown => "ctrl+j/↓",
            Self::OpenInBrowser => "o",
            Self::Copy => "y",
            Self::CopyLink => "Y",
            Self::AssignToMe => "a",
            Self::Unassign => "u",
            Self::Tag => "t",
            Self::Comment => "m",
            Self::ChangeColumn => "s",
            Self::Complete => "c",
            Self::ChangeToken => "T",
            Self::Sort => "S",
            Self::Filter => "f",
            Self::Sprint => "i",
            Self::Toggle => "space",
            Self::NextTab => "tab",
            Self::PrevTab => "shift+tab",
            Self::Help => "?",
            Self::NextLink => "l",
            Self::PrevLink => "L",
            Self::Vote => "v",
            Self::NextThread => "]",
            Self::PrevThread => "[",
            Self::ThreadStatus => "x",
            Self::PrActions => "e",
            Self::SwitchProject => "P",
            Self::Zoom => "z",
            Self::Top => "g/G",
            Self::Bottom => "G",
            Self::PageDown => "pgdn/pgup",
            Self::PageUp => "pgup",
            Self::HalfPageDown => "ctrl+d/u",
            Self::HalfPageUp => "ctrl+u",
            Self::JumpToPanel(_) => "1-6",
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
            Self::PrActions => "PR actions",
            Self::SwitchProject => "change project",
            Self::Zoom => "zoom",
            Self::Top => "top/bottom",
            Self::Bottom => "bottom",
            Self::PageDown => "page down/up",
            Self::PageUp => "page up",
            Self::HalfPageDown => "half page down/up",
            Self::HalfPageUp => "half page up",
            Self::JumpToPanel(_) => "go to box",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        Action::from_key(KeyEvent::new(code, modifiers))
    }

    fn plain(code: KeyCode) -> Option<Action> {
        key(code, KeyModifiers::NONE)
    }

    #[test]
    fn hjkl_and_arrows_move() {
        assert_eq!(plain(KeyCode::Char('j')), Some(Action::Down));
        assert_eq!(plain(KeyCode::Down), Some(Action::Down));
        assert_eq!(plain(KeyCode::Char('k')), Some(Action::Up));
        assert_eq!(plain(KeyCode::Up), Some(Action::Up));
    }

    #[test]
    fn ctrl_direction_changes_focus() {
        let ctrl = KeyModifiers::CONTROL;

        assert_eq!(key(KeyCode::Char('h'), ctrl), Some(Action::FocusLeft));
        assert_eq!(key(KeyCode::Char('l'), ctrl), Some(Action::FocusRight));
        assert_eq!(key(KeyCode::Char('k'), ctrl), Some(Action::FocusUp));
        assert_eq!(key(KeyCode::Char('j'), ctrl), Some(Action::FocusDown));
        assert_eq!(key(KeyCode::Left, ctrl), Some(Action::FocusLeft));
        assert_eq!(key(KeyCode::Right, ctrl), Some(Action::FocusRight));
        assert_eq!(key(KeyCode::Up, ctrl), Some(Action::FocusUp));
        assert_eq!(key(KeyCode::Down, ctrl), Some(Action::FocusDown));
    }

    #[test]
    fn lowercase_and_uppercase_s_differ() {
        assert_eq!(plain(KeyCode::Char('s')), Some(Action::ChangeColumn));
        assert_eq!(
            key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            Some(Action::Sort)
        );
    }

    #[test]
    fn tab_and_back_tab_switch_tabs() {
        assert_eq!(plain(KeyCode::Tab), Some(Action::NextTab));
        assert_eq!(
            key(KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(Action::PrevTab)
        );
    }

    #[test]
    fn quit_keys() {
        assert_eq!(plain(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Action::Quit)
        );
    }

    #[test]
    fn other_keys() {
        assert_eq!(plain(KeyCode::Esc), Some(Action::Cancel));
        assert_eq!(plain(KeyCode::Enter), Some(Action::Confirm));
        assert_eq!(
            key(KeyCode::Char('?'), KeyModifiers::SHIFT),
            Some(Action::Help)
        );
        assert_eq!(plain(KeyCode::Char(' ')), Some(Action::Toggle));
        assert_eq!(plain(KeyCode::Char('f')), Some(Action::Filter));
        assert_eq!(plain(KeyCode::Char('z')), Some(Action::Zoom));
    }

    #[test]
    fn unbound_key_is_ignored() {
        assert_eq!(plain(KeyCode::Char('Z')), None);
        assert_eq!(plain(KeyCode::F(1)), None);
        assert_eq!(plain(KeyCode::Char('0')), None);
        assert_eq!(plain(KeyCode::Char('7')), None);
    }

    #[test]
    fn navigation_keys() {
        let ctrl = KeyModifiers::CONTROL;

        assert_eq!(plain(KeyCode::Char('g')), Some(Action::Top));
        assert_eq!(
            key(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Action::Bottom)
        );
        assert_eq!(plain(KeyCode::PageDown), Some(Action::PageDown));
        assert_eq!(plain(KeyCode::PageUp), Some(Action::PageUp));
        assert_eq!(key(KeyCode::Char('d'), ctrl), Some(Action::HalfPageDown));
        assert_eq!(key(KeyCode::Char('u'), ctrl), Some(Action::HalfPageUp));
        assert_eq!(plain(KeyCode::Char('u')), Some(Action::Unassign));
        assert_eq!(plain(KeyCode::Char('1')), Some(Action::JumpToPanel(1)));
        assert_eq!(plain(KeyCode::Char('6')), Some(Action::JumpToPanel(6)));
    }

    #[test]
    fn help_entries_have_labels() {
        for action in HELP.iter().flat_map(|(_, actions)| actions.iter()) {
            assert!(!action.key_label().is_empty(), "{action:?}");
            assert!(!action.label().is_empty(), "{action:?}");
        }
    }
}
