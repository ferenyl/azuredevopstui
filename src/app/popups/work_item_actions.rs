use std::collections::HashSet;

use crossterm::event::KeyEvent;
use serde_json::{Value, json};

use crate::api::{IdentityRef, Person, WorkItem};
use crate::app::loads::Load;
use crate::app::message::Message;
use crate::app::{
    App, AssigneePicker, Detail, Popup, Selection, TextInput, TextTarget, WorkItemAction,
    WorkItemField,
};

const PRIORITIES: [&str; 4] = ["1", "2", "3", "4"];

/// A change to a work item field, sent to Azure DevOps.
#[derive(Debug, PartialEq)]
pub(in crate::app) enum WorkItemChange {
    State(String),
    Title(String),
    /// A unique name; empty unassigns.
    AssignTo(String),
    Priority(u8),
    Iteration(String),
}

impl WorkItemChange {
    /// The field reference name and its new value.
    fn field(&self) -> (&'static str, Value) {
        match self {
            Self::State(state) => ("System.State", json!(state)),
            Self::Title(title) => ("System.Title", json!(title)),
            Self::AssignTo(name) => ("System.AssignedTo", json!(name)),
            Self::Priority(priority) => ("Microsoft.VSTS.Common.Priority", json!(priority)),
            Self::Iteration(path) => ("System.IterationPath", json!(path)),
        }
    }
}

impl App {
    pub(in crate::app) fn open_work_item_actions(&mut self) {
        let Some(item) = self.current_work_item() else {
            return;
        };
        let mut actions = WorkItemAction::ALL.to_vec();
        actions.extend(
            WorkItemAction::FORWARDED
                .into_iter()
                .filter(|a| a.action().is_some_and(|action| self.can_in_main(action))),
        );
        let selection = Selection::new(
            actions
                .iter()
                .map(|a| self.with_key_hint(a.label(), a.action()))
                .collect(),
        );
        self.popup = Some(Popup::WorkItemActions {
            item: Box::new(item),
            actions,
            selection,
        });
    }

    pub(in crate::app) fn confirm_work_item_action(
        &mut self,
        item: Box<WorkItem>,
        actions: &[WorkItemAction],
        index: usize,
    ) {
        let Some(action) = actions.get(index).copied() else {
            return;
        };
        if let Some(key_action) = action.action() {
            self.apply(key_action);
            return;
        }
        match action {
            WorkItemAction::ChangeColumn
            | WorkItemAction::AssignToMe
            | WorkItemAction::Unassign
            | WorkItemAction::Tag
            | WorkItemAction::Comment
            | WorkItemAction::OpenInBrowser
            | WorkItemAction::CopyLink => {}
            WorkItemAction::ChangeState => {
                let Some(api) = self.api() else {
                    return;
                };
                self.spawn(async move {
                    let result = api.work_item_states(&item.fields.work_item_type).await;
                    Message::WorkItemStates { item, result }
                });
            }
            WorkItemAction::EditTitle => {
                let text = TextInput::new(item.fields.title.clone());
                self.popup = Some(Popup::Text {
                    target: TextTarget::WorkItemTitle(item),
                    text,
                });
            }
            WorkItemAction::AssignTo => {
                let people = self.known_people();
                if let Some(api) = self.api() {
                    let id = item.id;
                    self.spawn(async move {
                        let result = api.team_members().await;
                        Message::TeamMembers { item: id, result }
                    });
                }
                self.popup = Some(Popup::Assignee(AssigneePicker::new(item, people)));
            }
            WorkItemAction::Priority => {
                let priorities = PRIORITIES.map(String::from).to_vec();
                self.open_field_picker(item, WorkItemField::Priority, priorities, None);
            }
            WorkItemAction::Iteration => {
                let Some(api) = self.api() else {
                    return;
                };
                self.spawn(async move {
                    let result = api.iterations().await;
                    Message::WorkItemSprints { item, result }
                });
            }
            WorkItemAction::NewChild => {
                let Some(api) = self.api() else {
                    return;
                };
                self.spawn(async move {
                    let types = api.child_work_item_types().await;
                    Message::ChildWorkItemTypes {
                        parent: item,
                        types,
                    }
                });
            }
        }
    }

    /// Lists `values` with the item's value highlighted, or `default` when it has none.
    pub(in crate::app) fn open_field_picker(
        &mut self,
        item: Box<WorkItem>,
        field: WorkItemField,
        values: Vec<String>,
        default: Option<&str>,
    ) {
        let mut selection = Selection::new(values);
        if let Some(current) = field.value(&item).as_deref().or(default) {
            selection.preselect(current);
        }
        self.popup = Some(Popup::WorkItemField {
            item,
            field,
            selection,
        });
    }

    /// Sends nothing when the value stays the same.
    pub(in crate::app) fn confirm_work_item_field(
        &mut self,
        item: Box<WorkItem>,
        field: WorkItemField,
        selection: &Selection,
    ) {
        let Some(value) = selection.current() else {
            return;
        };
        if field.value(&item).as_deref() == Some(value) {
            return;
        }
        let change = match field {
            WorkItemField::State => WorkItemChange::State(value.into()),
            WorkItemField::Priority => match value.parse() {
                Ok(priority) => WorkItemChange::Priority(priority),
                Err(_) => return,
            },
            WorkItemField::Iteration => WorkItemChange::Iteration(value.into()),
        };
        self.send_work_item_update(item, change);
    }

    pub(in crate::app) fn confirm_work_item_title(&mut self, item: Box<WorkItem>, text: &str) {
        let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !title.is_empty() && title != item.fields.title {
            self.send_work_item_update(item, WorkItemChange::Title(title));
        }
    }

    /// Assigns the highlighted person; without a match, searches for people matching the query.
    pub(in crate::app) fn confirm_assignee(&mut self, picker: AssigneePicker) {
        if let Some(person) = picker.person() {
            let name = person.unique_name.clone();
            self.send_work_item_update(picker.item, WorkItemChange::AssignTo(name));
            return;
        }
        let query = picker.query.as_str().trim().to_string();
        let Some(api) = self.api().filter(|_| !query.is_empty()) else {
            return;
        };
        let item = picker.item;
        self.spawn(async move {
            let result = api.search_identities(&query).await;
            Message::Identities {
                item,
                query,
                result,
            }
        });
    }

    /// Unassigned, then the assignees of loaded work items and the people on loaded PRs, by name.
    fn known_people(&self) -> Vec<Person> {
        let me = self.user.iter().map(|user| Person {
            display_name: user.provider_display_name.clone(),
            unique_name: user.unique_name().to_string(),
        });
        let assignees = [&self.data.my_work_items, &self.data.ready_work_items]
            .into_iter()
            .flatten()
            .flatten()
            .chain(
                self.data
                    .inbox()
                    .iter()
                    .filter_map(|entry| match &entry.detail {
                        Detail::WorkItem(item) => Some(&**item),
                        _ => None,
                    }),
            )
            .filter_map(|item| item.fields.assigned_to.as_ref())
            .map(person);
        let on_prs = [self.data.my_prs(), self.data.other_prs()]
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|pr| {
                let reviewers = pr.reviewers.iter().map(|reviewer| Person {
                    display_name: reviewer.display_name.clone(),
                    unique_name: reviewer
                        .unique_name
                        .clone()
                        .unwrap_or_else(|| reviewer.display_name.clone()),
                });
                std::iter::once(person(&pr.created_by)).chain(reviewers)
            });
        let mut seen = HashSet::new();
        let mut people: Vec<Person> = me
            .chain(assignees)
            .chain(on_prs)
            .filter(|person| !person.unique_name.starts_with("vstfs:"))
            .filter(|person| seen.insert(person.unique_name.to_lowercase()))
            .collect();
        people.sort_by_key(|person| person.display_name.to_lowercase());
        people.insert(
            0,
            Person {
                display_name: "Unassigned".into(),
                unique_name: String::new(),
            },
        );
        people
    }

    pub(in crate::app) fn send_work_item_update(
        &mut self,
        item: Box<WorkItem>,
        change: WorkItemChange,
    ) {
        let Some(api) = self.api() else {
            return;
        };
        let id = item.id;
        let label = match &change {
            WorkItemChange::State(state) => format!("moving #{id} to {state}"),
            WorkItemChange::Title(_) => format!("renaming #{id}"),
            WorkItemChange::AssignTo(name) if name.is_empty() => format!("unassigning #{id}"),
            WorkItemChange::AssignTo(_) => format!("assigning #{id}"),
            WorkItemChange::Priority(priority) => format!("setting priority {priority} on #{id}"),
            WorkItemChange::Iteration(path) => {
                let sprint = path.rsplit('\\').next().unwrap_or(path);
                format!("moving #{id} to {sprint}")
            }
        };
        self.spawn_action(label, async move {
            let result = match change {
                WorkItemChange::AssignTo(name) => api.assign_work_item(id, &name).await,
                change => api.update_work_item(id, &[change.field()]).await,
            };
            Message::WorkItemUpdated {
                id,
                reload: &[Load::Sprint, Load::Mentions, Load::Detail],
                result,
            }
        });
    }
}

fn person(identity: &IdentityRef) -> Person {
    Person {
        display_name: identity.display_name.clone(),
        unique_name: identity
            .unique_name
            .clone()
            .unwrap_or_else(|| identity.display_name.clone()),
    }
}

impl AssigneePicker {
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
    use serde_json::json;

    use super::WorkItemChange;
    use crate::api::Person;
    use crate::app::message::Message;
    use crate::app::test_helpers::{app, assign_first_item_to, ctrl, press};
    use crate::app::{App, AssigneePicker, Popup, TextTarget, WorkItemField};
    use crate::test_support::work_item;

    fn open_menu(app: &mut App) {
        ctrl(app, 'j');
        press(app, KeyCode::Char('e'));
    }

    fn choose(app: &mut App, row: usize) {
        open_menu(app);
        for _ in 0..row {
            press(app, KeyCode::Char('j'));
        }
        press(app, KeyCode::Enter);
    }

    fn field_picker(app: &App) -> (WorkItemField, Vec<String>, Option<&str>) {
        let Some(Popup::WorkItemField {
            field, selection, ..
        }) = &app.popup
        else {
            panic!("field picker not open");
        };
        (*field, selection.items.clone(), selection.current())
    }

    #[test]
    fn e_on_a_work_item_opens_its_menu() {
        let mut app = app();

        open_menu(&mut app);

        let Some(Popup::WorkItemActions {
            item, selection, ..
        }) = &app.popup
        else {
            panic!("menu not open");
        };
        assert_eq!(item.id, 10);
        assert_eq!(
            selection.items,
            [
                "Change state",
                "Edit title",
                "Assign to…",
                "Set priority",
                "Move to sprint",
                "New child…",
                "Change column  (s)",
                "Assign to me  (a)",
                "Tag  (t)",
                "Comment  (m)",
                "Open in browser  (o)",
                "Copy link  (Y)"
            ]
        );
    }

    #[test]
    fn edit_title_prefills_the_title_and_skips_blank_or_unchanged() {
        let mut app = app();
        choose(&mut app, 1);
        assert!(matches!(
            &app.popup,
            Some(Popup::Text { target: TextTarget::WorkItemTitle(_), text }) if text.as_str() == "Story"
        ));

        let item = || Box::new(work_item(10, "Story", "User Story", "Active"));
        app.popup = None;
        app.confirm_work_item_title(item(), "  Story ");
        app.confirm_work_item_title(item(), "   ");

        assert!(app.popup.is_none());
        assert_eq!(app.activity.latest(), None);
    }

    #[test]
    fn priority_lists_one_to_four_with_the_current_one_highlighted() {
        let mut app = app();
        app.data.my_work_items.as_mut().unwrap()[0].fields.priority = Some(3);

        choose(&mut app, 3);

        let (field, items, current) = field_picker(&app);
        assert_eq!(field, WorkItemField::Priority);
        assert_eq!(items, ["1", "2", "3", "4"]);
        assert_eq!(current, Some("3"));
    }

    #[test]
    fn states_highlight_the_current_state_and_keeping_it_sends_nothing() {
        let mut app = app();
        let item = Box::new(work_item(10, "Story", "User Story", "Active"));

        app.handle_message(Message::WorkItemStates {
            item,
            result: Ok(vec!["New".into(), "Active".into(), "Resolved".into()]),
        });

        let (field, _, current) = field_picker(&app);
        assert_eq!(field, WorkItemField::State);
        assert_eq!(current, Some("Active"));
        press(&mut app, KeyCode::Enter);
        assert!(app.popup.is_none());
        assert_eq!(app.activity.latest(), None);
    }

    #[test]
    fn sprints_highlight_the_items_sprint_or_else_the_current_one() {
        let sprints = || {
            serde_json::from_value(json!([
                { "name": "S2", "path": "P\\S2" },
                { "name": "S1", "path": "P\\S1", "attributes": { "timeFrame": "current" } },
                { "name": "S0", "path": "P\\S0" }
            ]))
            .unwrap()
        };
        let mut app = app();
        let mut item = work_item(10, "Story", "User Story", "Active");

        app.handle_message(Message::WorkItemSprints {
            item: Box::new(item.clone()),
            result: Ok(sprints()),
        });
        assert_eq!(field_picker(&app).2, Some("P\\S1"));

        item.fields.iteration_path = Some("P\\S0".into());
        app.handle_message(Message::WorkItemSprints {
            item: Box::new(item),
            result: Ok(sprints()),
        });
        let (field, items, current) = field_picker(&app);
        assert_eq!(field, WorkItemField::Iteration);
        assert_eq!(items, ["P\\S2", "P\\S1", "P\\S0"]);
        assert_eq!(current, Some("P\\S0"));
    }

    #[test]
    fn each_change_sets_its_field() {
        let fields = [
            (
                WorkItemChange::State("Resolved".into()),
                "System.State",
                json!("Resolved"),
            ),
            (
                WorkItemChange::Title("New".into()),
                "System.Title",
                json!("New"),
            ),
            (
                WorkItemChange::AssignTo("a@b.c".into()),
                "System.AssignedTo",
                json!("a@b.c"),
            ),
            (
                WorkItemChange::Priority(1),
                "Microsoft.VSTS.Common.Priority",
                json!(1),
            ),
            (
                WorkItemChange::Iteration("P\\S1".into()),
                "System.IterationPath",
                json!("P\\S1"),
            ),
        ];
        for (change, name, value) in fields {
            assert_eq!(change.field(), (name, value), "{change:?}");
        }
    }

    fn people(app: &App) -> Vec<String> {
        let Some(Popup::Assignee(picker)) = &app.popup else {
            panic!("assignee picker not open");
        };
        picker.selection.items.clone()
    }

    #[test]
    fn assign_lists_unassigned_then_known_people_once_each() {
        let mut app = app();
        app.user = Some(crate::app::test_helpers::me());
        assign_first_item_to(&mut app, "someone-id");
        let item = &mut app.data.my_work_items.as_mut().unwrap()[1];
        item.fields.assigned_to = Some(
            serde_json::from_value(json!({
                "id": "anna", "displayName": "Anna", "uniqueName": "anna@example.com"
            }))
            .unwrap(),
        );

        choose(&mut app, 2);

        let people = people(&app);
        assert_eq!(people[0], "Unassigned");
        assert!(people.contains(&"Anna <anna@example.com>".to_string()));
        assert!(people.contains(&"Me".to_string()));
        assert!(people.contains(&"Someone".to_string()));
        let mut unique = people.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), people.len(), "{people:?}");
        let Some(Popup::Assignee(picker)) = &app.popup else {
            unreachable!();
        };
        assert_eq!(picker.selection.current(), Some("Someone"));
    }

    #[test]
    fn typing_filters_people_by_name_or_email() {
        let mut app = app();
        let people = vec![
            Person {
                display_name: "Unassigned".into(),
                unique_name: String::new(),
            },
            Person {
                display_name: "Anna".into(),
                unique_name: "anna@example.com".into(),
            },
            Person {
                display_name: "Bo".into(),
                unique_name: "bo@other.com".into(),
            },
        ];
        let item = Box::new(work_item(10, "Story", "User Story", "Active"));
        app.popup = Some(Popup::Assignee(AssigneePicker::new(item, people)));

        for c in "other".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        let Some(Popup::Assignee(picker)) = &app.popup else {
            panic!("assignee picker not open");
        };
        assert_eq!(picker.selection.items, ["Bo <bo@other.com>"]);
        assert_eq!(
            picker.person().map(|p| p.unique_name.as_str()),
            Some("bo@other.com")
        );
    }

    #[test]
    fn unknown_text_keeps_the_picker_until_people_are_found() {
        let mut app = app();
        choose(&mut app, 2);
        for c in "zed".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert!(people(&app).is_empty());

        press(&mut app, KeyCode::Enter);
        assert!(app.popup.is_none());
        assert_eq!(app.activity.latest(), None);

        let item = Box::new(work_item(10, "Story", "User Story", "Active"));
        app.handle_message(Message::Identities {
            item: item.clone(),
            query: "zed".into(),
            result: Ok(vec![Person {
                display_name: "Zed".into(),
                unique_name: "zed@example.com".into(),
            }]),
        });
        assert_eq!(people(&app), ["Zed <zed@example.com>"]);

        app.popup = None;
        app.handle_message(Message::Identities {
            item,
            query: "nobody".into(),
            result: Ok(Vec::new()),
        });
        assert_eq!(app.error.as_deref(), Some("no one found: nobody"));
    }

    #[test]
    fn new_child_picks_the_type_then_asks_for_the_title() {
        let mut app = app();
        let parent = Box::new(work_item(10, "Story", "User Story", "Active"));

        app.handle_message(Message::ChildWorkItemTypes {
            parent,
            types: vec!["Task".into(), "Test Bug".into()],
        });
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);

        assert!(matches!(
            &app.popup,
            Some(Popup::Text {
                target: TextTarget::NewWorkItem { kind, parent: Some(parent) },
                ..
            }) if kind == "Test Bug" && parent.id == 10
        ));
    }
}
