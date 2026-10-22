use crate::app::{App, Popup, Selection};

impl App {
    pub(in crate::app) fn open_pr_filter(&mut self) {
        let Some(config) = &self.config else {
            return;
        };
        let filter = &config.other_prs_filter;
        self.popup = Some(Popup::PrFilter {
            selection: Selection::new(vec!["Show approved".into(), "Show drafts".into()]),
            checked: vec![filter.show_approved, filter.show_drafts],
        });
    }

    pub(in crate::app) fn confirm_pr_filter(&mut self, checked: &[bool]) {
        let Some(config) = &mut self.config else {
            return;
        };
        let filter = &mut config.other_prs_filter;
        filter.show_approved = checked[0];
        filter.show_drafts = checked[1];
        self.data.show_approved = filter.show_approved;
        self.data.show_drafts = filter.show_drafts;
        let result = config.save();
        self.clamp_selections();
        if let Err(err) = result {
            self.report_error(err);
        }
    }
}
