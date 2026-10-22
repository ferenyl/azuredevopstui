use crate::app::message::Message;
use crate::app::{App, Popup};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
