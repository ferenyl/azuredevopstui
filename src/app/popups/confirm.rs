use crossterm::event::{KeyCode, KeyEvent};

use super::pr_actions::PrChange;
use crate::app::{App, ConfirmAction, Popup};

impl App {
    pub(super) fn confirm_action(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Complete {
                pr,
                options,
                auto_complete,
            } => self.send_completion(pr, options, auto_complete),
            ConfirmAction::Abandon(pr) => self.send_pr_update(pr, PrChange::Abandon),
            ConfirmAction::DiscardText { .. } => {}
        }
    }

    /// `y` confirms, `n` declines; true when the key was consumed.
    pub(super) fn handle_confirm_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('y' | 'Y') => self.confirm_popup(),
            KeyCode::Char('n' | 'N') => self.cancel_popup(),
            _ => return false,
        }
        true
    }

    /// Closes the open popup. A text popup with text asks first; declining that brings it back.
    pub(super) fn cancel_popup(&mut self) {
        self.popup = match self.popup.take() {
            Some(Popup::Text { target, text }) if !text.as_str().is_empty() => {
                Some(Popup::Confirm {
                    prompt: "Discard the text?".into(),
                    action: ConfirmAction::DiscardText { target, text },
                })
            }
            Some(Popup::Confirm {
                action: ConfirmAction::DiscardText { target, text },
                ..
            }) => Some(Popup::Text { target, text }),
            _ => None,
        };
    }

    pub(super) fn is_confirm_open(&self) -> bool {
        matches!(self.popup, Some(Popup::Confirm { .. }))
    }
}
