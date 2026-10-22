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

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::test_helpers::{app, press};
    use crate::app::{Panel, Popup};

    fn builds_app() -> crate::app::App {
        let mut app = app();
        app.focus = Panel::Builds;
        app.data.build_filter = "old".into();
        app.selections[Panel::Builds.index().unwrap()] = 2;
        app
    }

    #[test]
    fn filter_opens_with_current_text() {
        let mut app = builds_app();

        press(&mut app, KeyCode::Char('f'));

        assert!(matches!(&app.popup, Some(Popup::BuildFilter { text }) if text == "old"));
    }

    #[test]
    fn confirm_sets_trimmed_filter_and_resets_selection() {
        let mut app = builds_app();
        press(&mut app, KeyCode::Char('f'));
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        for c in " ci ".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert_eq!(app.data.build_filter, "ci");
        assert_eq!(app.selections[Panel::Builds.index().unwrap()], 0);
    }

    #[test]
    fn esc_keeps_old_filter() {
        let mut app = builds_app();
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('x'));

        press(&mut app, KeyCode::Esc);

        assert!(app.popup.is_none());
        assert_eq!(app.data.build_filter, "old");
        assert_eq!(app.selections[Panel::Builds.index().unwrap()], 2);
    }
}
