use crossterm::event::KeyEvent;

use crate::app::{App, SprintPicker};

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
        let taken = self.query.handle_key(key, false);
        if taken {
            self.filter();
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::api::Iteration;
    use crate::app::test_helpers::{app, press};
    use crate::app::{App, Popup, SprintPicker};

    fn sprint(path: &str) -> Iteration {
        Iteration {
            name: path.into(),
            path: path.into(),
            attributes: Default::default(),
        }
    }

    fn sprint_app() -> App {
        let mut app = app();
        let sprints = vec![sprint("Team\\Sprint 2"), sprint("Team\\Sprint 1")];
        app.popup = Some(Popup::Sprint(SprintPicker::new(sprints, None)));
        app
    }

    #[test]
    fn typing_filters_sprints() {
        let mut app = sprint_app();

        press(&mut app, KeyCode::Char('1'));

        let Some(Popup::Sprint(picker)) = &app.popup else {
            panic!("sprint picker not open");
        };
        assert_eq!(picker.query.as_str(), "1");
        assert_eq!(picker.selection.items, ["Team\\Sprint 1"]);
    }

    #[test]
    fn confirm_switches_sprint_and_clears_sprint_data() {
        let mut app = sprint_app();
        press(&mut app, KeyCode::Char('1'));

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert_eq!(app.sprint.as_deref(), Some("Team\\Sprint 1"));
        assert!(app.data.my_work_items.is_none());
        assert!(app.data.ready_work_items.is_none());
        assert!(app.data.sprint_name.is_none());
    }

    #[test]
    fn confirm_without_match_keeps_sprint() {
        let mut app = sprint_app();
        press(&mut app, KeyCode::Char('x'));

        press(&mut app, KeyCode::Enter);

        assert!(app.popup.is_none());
        assert!(app.sprint.is_none());
        assert!(app.data.my_work_items.is_some());
    }
}
