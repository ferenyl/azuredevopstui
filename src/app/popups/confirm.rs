use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, ConfirmAction, Popup};

impl App {
    pub(super) fn confirm_action(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Complete {
                pr,
                options,
                auto_complete,
            } => self.send_completion(pr, options, auto_complete),
        }
    }

    /// `y` confirms, `n` declines; true when the key was consumed.
    pub(super) fn handle_confirm_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('y' | 'Y') => self.confirm_popup(),
            KeyCode::Char('n' | 'N') => self.popup = None,
            _ => return false,
        }
        true
    }

    pub(super) fn is_confirm_open(&self) -> bool {
        matches!(self.popup, Some(Popup::Confirm { .. }))
    }
}
