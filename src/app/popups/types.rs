use crate::app::{App, Popup, Selection};

impl App {
    pub(in crate::app) fn open_types_picker(&mut self, mut types: Vec<String>) {
        let Some(config) = &self.config else {
            return;
        };
        for kind in &config.work_item_types {
            if !types.contains(kind) {
                types.push(kind.clone());
            }
        }
        let checked = types
            .iter()
            .map(|kind| config.work_item_types.is_empty() || config.work_item_types.contains(kind))
            .collect();
        self.popup = Some(Popup::Types {
            selection: Selection::new(types),
            checked,
        });
    }

    pub(in crate::app) fn confirm_types(&mut self, selection: Selection, checked: Vec<bool>) {
        let Some(config) = &mut self.config else {
            return;
        };
        config.work_item_types = checked_types(selection.items, &checked);
        if let Err(err) = config.save() {
            self.report_error(err);
            return;
        }
        self.data.my_work_items = None;
        self.data.ready_work_items = None;
        self.refresh();
    }
}

/// The ticked types; empty (meaning all) when every type is ticked.
fn checked_types(items: Vec<String>, checked: &[bool]) -> Vec<String> {
    if checked.iter().all(|checked| *checked) {
        return Vec::new();
    }
    items
        .into_iter()
        .zip(checked)
        .filter_map(|(kind, checked)| checked.then_some(kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::KeyCode;

    use crate::app::Popup;
    use crate::app::test_helpers::{app, press};

    #[test]
    fn checked_types_keep_ticked_types() {
        let types = checked_types(vec!["Bug".into(), "Task".into()], &[false, true]);

        assert_eq!(types, ["Task"]);
    }

    #[test]
    fn all_or_no_ticked_types_mean_all() {
        assert!(checked_types(vec!["Bug".into(), "Task".into()], &[true, true]).is_empty());
        assert!(checked_types(vec!["Bug".into()], &[false]).is_empty());
    }

    #[test]
    fn types_picker_ticks_configured_types() {
        let mut app = app();
        app.config.as_mut().unwrap().work_item_types = vec!["Bug".into(), "Retired".into()];

        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        let Some(Popup::Types { selection, checked }) = &app.popup else {
            panic!("types picker not open");
        };
        assert_eq!(selection.items, ["Bug", "Task", "Retired"]);
        assert_eq!(checked, &[true, false, true]);
    }

    #[test]
    fn types_picker_ticks_all_without_filter() {
        let mut app = app();

        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        assert!(
            matches!(&app.popup, Some(Popup::Types { checked, .. }) if checked == &[true, true])
        );
    }

    #[test]
    fn space_toggles_type() {
        let mut app = app();
        app.open_types_picker(vec!["Bug".into(), "Task".into()]);

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char(' '));

        assert!(
            matches!(&app.popup, Some(Popup::Types { checked, .. }) if checked == &[true, false])
        );
    }
}
