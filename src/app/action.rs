use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
}

impl Action {
    pub fn from_key(key: KeyEvent) -> Option<Self> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => Some(Self::Quit),
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Self::Quit),
            _ => None,
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            Self::Quit => "q",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Quit => "quit",
        }
    }
}
