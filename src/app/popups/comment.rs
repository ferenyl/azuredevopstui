use crate::app::message::Message;
use crate::app::{App, Popup, TextInput};

impl App {
    pub(in crate::app) fn open_comment(&mut self) {
        if let Some(item) = self.current_work_item() {
            self.popup = Some(Popup::Comment {
                work_item_id: item.id,
                text: TextInput::default(),
            });
        }
    }

    pub(in crate::app) fn confirm_comment(&mut self, id: u32, text: &str) {
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

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::test_helpers::{app, press};
    use crate::app::{Popup, TextInput};

    #[test]
    fn m_opens_comment_for_selected_work_item() {
        let mut app = app();
        app.focus = crate::app::Panel::MyWorkItems;

        press(&mut app, KeyCode::Char('m'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { work_item_id: 10, text }) if text.as_str().is_empty()
        ));
    }

    #[test]
    fn blank_comment_is_dropped_on_confirm() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: TextInput::default(),
        });
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert!(app.error.is_none());
    }
}
