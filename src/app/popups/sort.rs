use crate::app::{App, Popup, Selection, SortKind};
use crate::config::{PrSort, WorkItemSort};

impl App {
    pub(in crate::app) fn open_sort_picker(&mut self) {
        let (Some(kind), Some(config)) = (self.focus.sort_kind(), &self.config) else {
            return;
        };
        let (labels, selected): (Vec<String>, _) = match kind {
            SortKind::PullRequests => (
                PrSort::ALL.iter().map(|s| s.label().into()).collect(),
                PrSort::ALL
                    .iter()
                    .position(|s| *s == config.sort.pull_requests),
            ),
            SortKind::WorkItems => (
                WorkItemSort::ALL.iter().map(|s| s.label().into()).collect(),
                WorkItemSort::ALL
                    .iter()
                    .position(|s| *s == config.sort.work_items),
            ),
        };
        let mut selection = Selection::new(labels);
        selection.selected = selected.unwrap_or(0);
        self.popup = Some(Popup::Sort { kind, selection });
    }

    pub(in crate::app) fn confirm_sort(&mut self, kind: SortKind, index: usize) {
        let Some(config) = &mut self.config else {
            return;
        };
        match kind {
            SortKind::PullRequests => {
                if let Some(sort) = PrSort::ALL.get(index) {
                    config.sort.pull_requests = *sort;
                }
            }
            SortKind::WorkItems => {
                if let Some(sort) = WorkItemSort::ALL.get(index) {
                    config.sort.work_items = *sort;
                }
            }
        }
        self.data.sort(&config.sort);
        let result = config.save();
        self.clamp_selections();
        if let Err(err) = result {
            self.report_error(err);
        }
    }
}
