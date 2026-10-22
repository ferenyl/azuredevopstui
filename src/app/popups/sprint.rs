use crate::app::{App, SprintPicker};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    pub(in crate::app) fn confirm_sprint(&mut self, picker: SprintPicker) {
        let Some(path) = picker.selection.current() else {
            return;
        };
        self.sprint = Some(path.to_string());
        self.data.my_work_items = None;
        self.data.ready_work_items = None;
        self.data.sprint_name = None;
        self.load_sprint();
    }
}

impl SprintPicker {
    /// Edits the query; true when the key was consumed.
    pub(super) fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
                self.filter();
                true
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.filter();
                true
            }
            _ => false,
        }
    }
}
