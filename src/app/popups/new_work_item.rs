use crate::api::WorkItem;
use crate::app::message::Message;
use crate::app::{App, Popup, Selection, TextInput, TextTarget};

impl App {
    /// Asks for the type first, then the title.
    pub(in crate::app) fn new_work_item(&mut self) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move { Message::NewWorkItemTypes(api.work_item_types().await) });
    }

    /// The configured work item types first, then the others.
    pub(in crate::app) fn open_new_work_item_picker(&mut self, mut types: Vec<String>) {
        let configured = self
            .config
            .as_ref()
            .map(|config| config.work_item_types.as_slice())
            .unwrap_or_default();
        types.sort_by_key(|kind| {
            configured
                .iter()
                .position(|c| c == kind)
                .unwrap_or(usize::MAX)
        });
        self.popup = Some(Popup::NewWorkItem {
            selection: Selection::new(types),
        });
    }

    pub(in crate::app) fn confirm_new_work_item_type(&mut self, selection: &Selection) {
        let Some(kind) = selection.current() else {
            return;
        };
        self.popup = Some(Popup::Text {
            target: TextTarget::NewWorkItem {
                kind: kind.into(),
                parent: None,
            },
            text: TextInput::default(),
        });
    }

    /// Creates the item assigned to the user: a child in the sprint and area of `parent`, or
    /// otherwise in the shown sprint and the team's default area.
    pub(in crate::app) fn confirm_new_work_item(
        &mut self,
        kind: String,
        parent: Option<Box<WorkItem>>,
        text: &str,
    ) {
        let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let (Some(api), Some(user)) = (self.api(), self.user.as_ref()) else {
            return;
        };
        if title.is_empty() {
            return;
        }
        let assignee = user.unique_name().to_string();
        let (iteration, area, parent) = match parent {
            Some(parent) => (
                parent.fields.iteration_path,
                parent.fields.area_path,
                Some(parent.id),
            ),
            None => (self.sprint.clone(), None, None),
        };
        let label = match parent {
            Some(id) => format!("creating a task under #{id}"),
            None => format!("creating a {kind}"),
        };
        self.spawn_action(label, async move {
            let result = api
                .create_work_item(
                    &kind,
                    &title,
                    &assignee,
                    iteration.as_deref(),
                    area.as_deref(),
                    parent,
                )
                .await;
            Message::WorkItemCreated { parent, result }
        });
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::app::message::Message;
    use crate::app::test_helpers::{app, press};
    use crate::app::{Popup, TextTarget};

    #[test]
    fn types_list_the_configured_ones_first_and_ask_for_the_title() {
        let mut app = app();
        let mut config = (**app.config.as_ref().unwrap()).clone();
        config.work_item_types = vec!["User Story".into(), "Bug".into()];
        app.config = Some(config.into());

        app.handle_message(Message::NewWorkItemTypes(Ok(vec![
            "Bug".into(),
            "Epic".into(),
            "Task".into(),
            "User Story".into(),
        ])));

        let Some(Popup::NewWorkItem { selection }) = &app.popup else {
            panic!("type picker not open");
        };
        assert_eq!(selection.items, ["User Story", "Bug", "Epic", "Task"]);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            &app.popup,
            Some(Popup::Text {
                target: TextTarget::NewWorkItem { kind, parent: None },
                ..
            }) if kind == "Bug"
        ));
    }

    #[test]
    fn created_item_is_noticed_with_its_id() {
        let mut app = app();

        app.handle_message(Message::WorkItemCreated {
            parent: None,
            result: Ok(900),
        });

        assert_eq!(app.notice(), Some("created #900"));
    }

    #[test]
    fn esc_on_the_type_picker_closes_it() {
        let mut app = app();
        app.handle_message(Message::NewWorkItemTypes(Ok(vec!["Bug".into()])));

        press(&mut app, KeyCode::Esc);

        assert!(app.popup.is_none());
    }
}
