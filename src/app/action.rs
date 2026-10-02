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
    AssignToMe,
    ChangeColumn,
    ChangeToken,
    Sort,
    FilterTypes,
    Toggle,
    NextTab,
    PrevTab,
    Help,
}

/// Actions listed in the help popup.
pub const HELP: [Action; 14] = [
    Action::Down,
    Action::FocusRight,
    Action::NextTab,
    Action::Confirm,
    Action::Sort,
    Action::FilterTypes,
    Action::ChangeColumn,
    Action::AssignToMe,
    Action::OpenInBrowser,
    Action::Reload,
    Action::ChangeToken,
    Action::Help,
    Action::Cancel,
    Action::Quit,
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
            (KeyCode::Char('a'), _) => Some(Self::AssignToMe),
            (KeyCode::Char('s'), _) => Some(Self::ChangeColumn),
            (KeyCode::Char('S'), _) => Some(Self::Sort),
            (KeyCode::Char('f'), _) => Some(Self::FilterTypes),
            (KeyCode::Char(' '), _) => Some(Self::Toggle),
            (KeyCode::Tab, _) => Some(Self::NextTab),
            (KeyCode::BackTab, _) => Some(Self::PrevTab),
            (KeyCode::Char('t'), _) => Some(Self::ChangeToken),
            (KeyCode::Char('?'), _) => Some(Self::Help),
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
            Self::AssignToMe => "a",
            Self::ChangeColumn => "s",
            Self::ChangeToken => "t",
            Self::Sort => "S",
            Self::FilterTypes => "f",
            Self::Toggle => "space",
            Self::NextTab => "tab",
            Self::PrevTab => "shift+tab",
            Self::Help => "?",
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
            Self::AssignToMe => "assign to me",
            Self::ChangeColumn => "change status",
            Self::ChangeToken => "set PAT",
            Self::Sort => "sort",
            Self::FilterTypes => "work item types",
            Self::Toggle => "toggle",
            Self::NextTab => "next tab",
            Self::PrevTab => "previous tab",
            Self::Help => "help",
        }
    }
}
