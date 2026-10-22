use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::message::Message;
use crate::app::{App, Popup};

impl App {
    pub(in crate::app) fn open_comment(&mut self) {
        if let Some(item) = self.current_work_item() {
            self.popup = Some(Popup::Comment {
                work_item_id: item.id,
                text: String::new(),
            });
        }
    }

    pub(in crate::app) fn confirm_comment(&mut self, id: u32, text: String) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        // Markdown joins single newlines; trailing spaces make them line breaks.
        let text = text.replace('\n', "  \n");
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            let result = client
                .add_work_item_comment(&config.organization, &config.project, id, &text)
                .await;
            Message::WorkItemUpdated { id, result }
        });
    }
}

/// Edits the comment; shift/alt+enter adds a line. True when the key was consumed.
pub(super) fn handle_key(text: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter
            if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            text.push('\n');
            true
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::Popup;
    use crate::app::test_helpers::{app, press};

    #[test]
    fn shift_or_alt_enter_adds_a_line() {
        let mut text = String::from("a");

        assert!(handle_key(
            &mut text,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
        ));
        assert!(handle_key(
            &mut text,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
        ));

        assert_eq!(text, "a\n\n");
    }

    #[test]
    fn plain_enter_and_ctrl_keys_are_not_taken() {
        let mut text = String::from("a");

        assert!(!handle_key(
            &mut text,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
        assert!(!handle_key(
            &mut text,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
        ));

        assert_eq!(text, "a");
    }

    #[test]
    fn m_opens_comment_for_selected_work_item() {
        let mut app = app();
        app.focus = crate::app::Panel::MyWorkItems;

        press(&mut app, KeyCode::Char('m'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { work_item_id: 10, text }) if text.is_empty()
        ));
    }

    #[test]
    fn blank_comment_is_dropped_on_confirm() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: String::new(),
        });
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }
}
