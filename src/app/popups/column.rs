use crate::api::{Board, WorkItem};
use crate::app::message::Message;
use crate::app::{App, ColumnPicker, Popup, Selection};

impl App {
    pub(in crate::app) fn open_column_picker(
        &mut self,
        item: WorkItem,
        board: &Board,
        unassign: bool,
    ) {
        let Some(targets) = board.targets(&item.fields.work_item_type) else {
            self.error = Some(format!(
                "{} items are not on the board",
                item.fields.work_item_type
            ));
            return;
        };
        let labels = targets.iter().map(|t| t.label.clone()).collect();
        let mut selection = Selection::new(labels);
        let done = item.fields.board_column_done.unwrap_or(false);
        let current = item.fields.board_column.as_deref();
        selection.selected = targets
            .iter()
            .position(|t| Some(t.column.as_str()) == current && t.done == done)
            .or_else(|| {
                targets
                    .iter()
                    .position(|t| Some(t.column.as_str()) == current)
            })
            .unwrap_or(0);
        self.popup = Some(Popup::Column(ColumnPicker {
            work_item_id: item.id,
            targets,
            selection,
            unassign,
        }));
    }

    pub(in crate::app) fn confirm_column(&mut self, picker: ColumnPicker) {
        let (Some(board), Some(client), Some(config)) =
            (self.board.clone(), self.client.clone(), self.config.clone())
        else {
            return;
        };
        let Some(target) = picker.targets.get(picker.selection.selected).cloned() else {
            return;
        };
        let (id, unassign) = (picker.work_item_id, picker.unassign);
        self.spawn(async move {
            let (organization, project) = (&config.organization, &config.project);
            let result = if unassign {
                client
                    .unassign_work_item(organization, project, id, &board, &target)
                    .await
            } else {
                client
                    .move_work_item(organization, project, id, &board, &target)
                    .await
            };
            Message::WorkItemUpdated { id, result }
        });
    }
}
