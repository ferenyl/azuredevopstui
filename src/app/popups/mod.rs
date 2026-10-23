mod build_actions;
mod build_filter;
mod column;
mod comment;
mod complete;
mod confirm;
mod pr_actions;
mod pr_filter;
mod sort;
mod sprint;
mod tags;
mod thread_status;
mod types;
mod vote;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
            Some(Popup::PrActions {
                pr,
                actions,
                selection,
            }) => self.confirm_pr_action(pr, &actions, selection.selected),
            Some(Popup::BuildActions {
                build,
                actions,
                selection,
            }) => self.confirm_build_action(build, &actions, selection.selected),
            Some(Popup::RemoveReviewer { pr, selection }) => {
                self.confirm_remove_reviewer(pr, selection.selected)
            }
            Some(Popup::Errors { .. }) | None => {}
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
            Some(Popup::Text { text, .. }) => {
                if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.edit_request = Some(text.as_str().to_string());
                    return true;
                }
                text.handle_key(key, true)
            }
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
                Action::Cancel => self.cancel_popup(),
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return true;
        }
        if let Some(Popup::Errors { first }) = &mut self.popup {
            let last = self.activity.error_count().saturating_sub(1);
            match action {
                Action::Down => *first = (*first + 1).min(last),
                Action::Up => *first = first.saturating_sub(1),
                Action::Confirm | Action::Cancel => self.popup = None,
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
            Popup::Text { .. }
            | Popup::BuildFilter { .. }
            | Popup::Confirm { .. }
            | Popup::Errors { .. } => {
                return true;
            }
            Popup::Sort { selection, .. }
            | Popup::Vote { selection, .. }
            | Popup::ThreadStatus { selection, .. }
            | Popup::PrActions { selection, .. }
            | Popup::BuildActions { selection, .. }
            | Popup::RemoveReviewer { selection, .. }
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

    use crate::app::test_helpers::{app, ctrl, press};
    use crate::app::{ConfirmAction, Popup, TextInput, TextTarget};

    fn open_text(app: &mut crate::app::App, text: &str) {
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(1),
            text: TextInput::new(text),
        });
    }

    #[test]
    fn esc_on_text_asks_and_n_or_esc_brings_it_back_with_the_cursor() {
        let mut app = app();
        open_text(&mut app, "draft");
        press(&mut app, KeyCode::Left);

        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.popup,
            Some(Popup::Confirm {
                action: ConfirmAction::DiscardText { .. },
                ..
            })
        ));
        press(&mut app, KeyCode::Char('n'));
        let Some(Popup::Text { text, .. }) = &app.popup else {
            panic!("text popup not restored");
        };
        assert_eq!(text.split(), ("draf", "t"));

        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.popup, Some(Popup::Text { .. })));
    }

    #[test]
    fn y_discards_the_text_and_empty_text_closes_at_once() {
        let mut app = app();
        open_text(&mut app, "draft");
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('y'));
        assert!(app.popup.is_none());

        open_text(&mut app, "");
        press(&mut app, KeyCode::Esc);
        assert!(app.popup.is_none());
    }

    #[test]
    fn ctrl_e_requests_the_editor_with_the_text() {
        let mut app = app();
        open_text(&mut app, "draft");

        ctrl(&mut app, 'e');

        assert_eq!(app.edit_request.as_deref(), Some("draft"));
    }

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
