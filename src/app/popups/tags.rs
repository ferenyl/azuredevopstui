use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};

use crate::api::WorkItem;
use crate::app::loads::Load;
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
        let mut picker = picker;
        if !picker.toggled {
            picker.toggle();
        }
        if picker.is_unchanged() {
            return;
        }
        let tags = picker.pending.clone();
        let added: Vec<&String> = tags.iter().filter(|tag| !picker.has(tag)).collect();
        let (Some(api), Some(config)) = (self.api(), &mut self.config) else {
            return;
        };
        let config = Arc::make_mut(config);
        let id = picker.work_item_id;
        if !added.is_empty() {
            for tag in added.into_iter().rev() {
                config.recent_tags.retain(|r| !r.eq_ignore_ascii_case(tag));
                config.recent_tags.insert(0, tag.clone());
            }
            config.recent_tags.truncate(MAX_RECENT_TAGS);
            let result = config.save();
            if let Err(err) = result {
                self.report_error(err);
            }
        }
        self.spawn_action(format!("tagging #{id}"), async move {
            let result = api.set_work_item_tags(id, &tags).await;
            Message::WorkItemUpdated {
                id,
                reload: &[Load::Mentions, Load::Detail],
                result,
            }
        });
    }
}

impl TagPicker {
    /// Edits the query; true when the key was consumed.
    pub(super) fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Tab {
            self.toggle();
            return true;
        }
        let taken = self.query.handle_key(key, false);
        if taken {
            self.filter();
        }
        taken
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

    fn pending(app: &crate::app::App) -> Vec<String> {
        let Some(Popup::Tags(picker)) = &app.popup else {
            panic!("tag picker not open");
        };
        picker.pending.clone()
    }

    #[test]
    fn tab_toggles_tags_in_and_out_of_the_pending_set() {
        let mut app = tag_app();

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Tab);
        assert_eq!(pending(&app), ["backend", "frontend"]);

        press(&mut app, KeyCode::Tab);
        assert_eq!(pending(&app), ["backend"]);
    }

    #[test]
    fn tab_on_a_new_tag_adds_it_and_clears_the_query() {
        let mut app = tag_app();
        for c in "ops".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Tab);

        let Some(Popup::Tags(picker)) = &app.popup else {
            panic!("tag picker not open");
        };
        assert_eq!(picker.pending, ["ops"]);
        assert_eq!(picker.query.as_str(), "");
        assert_eq!(picker.selection.items, ["ops", "backend", "frontend"]);
        assert!(!picker.is_new(0));
        assert!(picker.is_checked(0));
    }

    #[test]
    fn cursor_follows_the_tag_when_toggling_reorders_rows() {
        let mut app = tag_app();
        press(&mut app, KeyCode::Down);

        press(&mut app, KeyCode::Tab);

        let Some(Popup::Tags(picker)) = &app.popup else {
            panic!("tag picker not open");
        };
        assert_eq!(picker.selection.items, ["frontend", "backend"]);
        assert_eq!(picker.selection.current(), Some("frontend"));
    }

    #[test]
    fn pending_equal_to_current_is_unchanged_ignoring_case_and_order() {
        let mut picker = TagPicker::new(10, vec!["Ops".into(), "web".into()], Vec::new());
        picker.pending = vec!["WEB".into(), "ops".into()];

        assert!(picker.is_unchanged());

        picker.pending.pop();
        assert!(!picker.is_unchanged());
    }

    #[test]
    fn tab_on_a_current_tag_and_back_is_unchanged() {
        let mut picker = TagPicker::new(10, vec!["web".into()], Vec::new());

        picker.toggle();
        assert!(!picker.is_unchanged());
        picker.toggle();

        assert!(picker.toggled);
        assert!(picker.is_unchanged());
    }

    #[test]
    fn t_opens_the_picker_on_an_item_assigned_to_someone_else() {
        let mut app = app();
        crate::app::test_helpers::assign_first_item_to(&mut app, "someone-else");
        crate::app::test_helpers::ctrl(&mut app, 'j');

        press(&mut app, KeyCode::Char('t'));

        assert!(app.error.is_none(), "{:?}", app.error);
    }

    #[test]
    fn ctrl_keys_are_not_taken_by_the_query() {
        let mut picker = TagPicker::new(10, Vec::new(), vec!["backend".into()]);

        let taken = picker.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));

        assert!(!taken);
        assert_eq!(picker.query.as_str(), "");
    }
}
