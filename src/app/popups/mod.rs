mod build_filter;
mod column;
mod comment;
mod complete;
mod confirm;
mod pr_filter;
mod sort;
mod sprint;
mod tags;
mod thread_status;
mod types;
mod vote;

use crossterm::event::KeyEvent;

use super::{Action, App, CompleteRow, Popup};

impl App {
    pub(super) fn confirm_popup(&mut self) {
        match self.popup.take() {
            Some(Popup::Column(picker)) => self.confirm_column(picker),
            Some(Popup::Complete(picker)) => self.confirm_complete(picker),
            Some(Popup::Tags(picker)) => self.confirm_tags(picker),
            Some(Popup::Sprint(picker)) => self.confirm_sprint(picker),
            Some(Popup::Text { target, text }) => self.confirm_text(target, text.as_str()),
            Some(Popup::Sort { kind, selection }) => self.confirm_sort(kind, selection.selected),
            Some(Popup::Types { selection, checked }) => self.confirm_types(selection, checked),
            Some(Popup::PrFilter { checked, .. }) => self.confirm_pr_filter(&checked),
            Some(Popup::BuildFilter { text }) => self.confirm_build_filter(text.as_str()),
            Some(Popup::Vote { pr, selection }) => self.confirm_vote(pr, selection.selected),
            Some(Popup::Confirm { action, .. }) => self.confirm_action(action),
            Some(Popup::ThreadStatus {
                pr,
                thread_id,
                selection,
            }) => self.confirm_thread_status(pr, thread_id, selection.selected),
            None => {}
        }
    }

    /// Lets the open popup's text input take the key; true when consumed.
    pub(super) fn handle_popup_key(&mut self, key: KeyEvent) -> bool {
        if self.is_confirm_open() {
            return self.handle_confirm_key(key);
        }
        match &mut self.popup {
            Some(Popup::Complete(picker)) if picker.row() == CompleteRow::Message => {
                picker.message.handle_key(key, false)
            }
            Some(Popup::Tags(picker)) => picker.handle_key(key),
            Some(Popup::Sprint(picker)) => picker.handle_key(key),
            Some(Popup::Text { text, .. }) => text.handle_key(key, true),
            Some(Popup::BuildFilter { text }) => text.handle_key(key, false),
            _ => false,
        }
    }

    /// Applies an action to the open popup; true when a popup is open.
    pub(super) fn apply_popup(&mut self, action: Action) -> bool {
        if matches!(
            self.popup,
            Some(Popup::Text { .. } | Popup::BuildFilter { .. } | Popup::Confirm { .. })
        ) {
            match action {
                Action::Confirm => self.confirm_popup(),
                Action::Cancel => self.popup = None,
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return true;
        }
        let Some(popup) = &mut self.popup else {
            return false;
        };
        let selection = match popup {
            Popup::Column(picker) => &mut picker.selection,
            Popup::Complete(picker) => &mut picker.selection,
            Popup::Tags(picker) => &mut picker.selection,
            Popup::Sprint(picker) => &mut picker.selection,
            Popup::Text { .. } | Popup::BuildFilter { .. } | Popup::Confirm { .. } => {
                return true;
            }
            Popup::Sort { selection, .. }
            | Popup::Vote { selection, .. }
            | Popup::ThreadStatus { selection, .. }
            | Popup::Types { selection, .. }
            | Popup::PrFilter { selection, .. } => selection,
        };
        match action {
            Action::Toggle => {
                if let Popup::Complete(picker) = popup {
                    picker.toggle();
                    picker.selection.items = picker.items();
                }
                if let Popup::Types { selection, checked } | Popup::PrFilter { selection, checked } =
                    popup
                    && let Some(checked) = checked.get_mut(selection.selected)
                {
                    *checked = !*checked;
                }
            }
            Action::Up => selection.previous(),
            Action::Down => selection.next(),
            Action::Confirm => self.confirm_popup(),
            Action::Cancel => self.popup = None,
            Action::Quit => self.should_quit = true,
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::Popup;
    use crate::app::test_helpers::{app, press};

    #[test]
    fn popup_selection_moves_with_j_and_k() {
        let mut app = app();
        press(&mut app, KeyCode::Char('S'));

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));

        let Some(Popup::Sort { selection, .. }) = &app.popup else {
            panic!("sort picker not open");
        };
        assert_eq!(selection.current(), Some("oldest"));
        assert_eq!(app.selections[1], 0);
    }
}
