use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::api::WorkItem;
use crate::app::message::Message;
use crate::app::{App, Popup, TagPicker};

const MAX_RECENT_TAGS: usize = 20;

impl App {
    pub(in crate::app) fn open_tag_picker(&mut self, item: &WorkItem, mut tags: Vec<String>) {
        let recent = self
            .config
            .as_ref()
            .map(|config| config.recent_tags.as_slice())
            .unwrap_or_default();
        tags.sort_by_key(|tag| {
            recent
                .iter()
                .position(|r| r.eq_ignore_ascii_case(tag))
                .unwrap_or(usize::MAX)
        });
        let current = item
            .fields
            .tags
            .as_deref()
            .unwrap_or_default()
            .split(';')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(String::from)
            .collect();
        self.popup = Some(Popup::Tags(TagPicker::new(item.id, current, tags)));
    }

    pub(in crate::app) fn confirm_tags(&mut self, picker: TagPicker) {
        let Some(tag) = picker
            .selection
            .items
            .get(picker.selection.selected)
            .cloned()
        else {
            return;
        };
        let (Some(client), Some(config)) = (self.client.clone(), &mut self.config) else {
            return;
        };
        let (organization, project) = (config.organization.clone(), config.project.clone());
        let id = picker.work_item_id;
        let mut tags = picker.current.clone();
        if picker.has(&tag) {
            tags.retain(|current| !current.eq_ignore_ascii_case(&tag));
        } else {
            config.recent_tags.retain(|r| !r.eq_ignore_ascii_case(&tag));
            config.recent_tags.insert(0, tag.clone());
            config.recent_tags.truncate(MAX_RECENT_TAGS);
            let result = config.save();
            if let Err(err) = result {
                self.report_error(err);
            }
            tags.push(tag);
        }
        self.spawn(async move {
            let result = client
                .set_work_item_tags(&organization, &project, id, &tags)
                .await;
            Message::WorkItemUpdated { id, result }
        });
    }
}

impl TagPicker {
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::test_helpers::{app, press};
    use crate::app::{Popup, TagPicker};

    fn tag_app() -> crate::app::App {
        let mut app = app();
        app.popup = Some(Popup::Tags(TagPicker::new(
            10,
            Vec::new(),
            vec!["backend".into(), "frontend".into()],
        )));
        app
    }

    fn items(app: &crate::app::App) -> Vec<String> {
        let Some(Popup::Tags(picker)) = &app.popup else {
            panic!("tag picker not open");
        };
        picker.selection.items.clone()
    }

    #[test]
    fn typing_filters_tags_and_backspace_widens() {
        let mut app = tag_app();

        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(items(&app), ["frontend", "fr"]);

        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(items(&app), ["backend", "frontend"]);
    }

    #[test]
    fn unknown_query_is_offered_as_new_tag() {
        let mut app = tag_app();

        for c in "ops".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        assert_eq!(items(&app), ["ops"]);
    }

    #[test]
    fn ctrl_keys_are_not_taken_by_the_query() {
        let mut picker = TagPicker::new(10, Vec::new(), vec!["backend".into()]);

        let taken = picker.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));

        assert!(!taken);
        assert_eq!(picker.query, "");
    }
}
