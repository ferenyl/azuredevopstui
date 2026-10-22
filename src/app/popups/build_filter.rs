use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Panel, Popup};

impl App {
    pub(in crate::app) fn open_build_filter(&mut self) {
        self.popup = Some(Popup::BuildFilter {
            text: self.data.build_filter.clone(),
        });
    }

    pub(in crate::app) fn confirm_build_filter(&mut self, text: &str) {
        self.data.build_filter = text.trim().to_string();
        if let Some(index) = Panel::Builds.index() {
            self.selections[index] = 0;
        }
    }
}

/// Edits the filter text; true when the key was consumed.
pub(super) fn handle_key(text: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            text.push(c);
            true
        }
        KeyCode::Backspace => {
            text.pop();
            true
        }
        _ => false,
    }
}
