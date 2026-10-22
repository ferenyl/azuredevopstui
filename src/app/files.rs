use anyhow::Result;

use crate::api::{ChangedFile, PullRequestChanges};
use crate::diff::FileDiff;

use super::loads::Load;
use super::message::Message;
use super::{App, Detail, DetailTab};

/// The changed files of the shown pull request, loaded when its Files tab is first shown.
pub struct Files {
    pub pr_id: u32,
    /// `None` while loading.
    pub changes: Option<PullRequestChanges>,
    /// Why the changes could not be loaded; they are loaded again next time the tab is shown.
    pub error: Option<String>,
    pub selected: usize,
    /// The file whose diff is shown, by index, with the diff once loaded.
    pub diff: Option<(usize, Option<FileDiff>)>,
}

impl Files {
    fn new(pr_id: u32) -> Self {
        Self {
            pr_id,
            changes: None,
            error: None,
            selected: 0,
            diff: None,
        }
    }

    pub fn list(&self) -> &[ChangedFile] {
        self.changes
            .as_ref()
            .map_or(&[], |changes| changes.files.as_slice())
    }

    fn select(&mut self, down: bool) {
        let last = self.list().len().saturating_sub(1);
        self.selected = if down {
            (self.selected + 1).min(last)
        } else {
            self.selected.saturating_sub(1)
        };
    }
}

impl App {
    /// Starts loading the changed files of the shown PR unless they are loaded or loading.
    pub(super) fn load_files(&mut self) {
        let Some(Detail::PullRequest(pr)) = &self.detail else {
            return;
        };
        if self
            .files
            .as_ref()
            .is_some_and(|files| files.pr_id == pr.pull_request_id && files.error.is_none())
        {
            return;
        }
        let Some(api) = self.api() else {
            return;
        };
        let (id, pr) = (pr.pull_request_id, pr.clone());
        self.files = Some(Files::new(id));
        self.spawn_load(Load::Files, async move {
            let result = api.pull_request_changes(&pr).await;
            Message::PullRequestChanges { id, result }
        });
    }

    pub(super) fn clear_files(&mut self) {
        self.loads.cancel(Load::Files);
        self.loads.cancel(Load::Diff);
        self.files = None;
    }

    /// The file list when it is shown, without an open diff.
    fn shown_file_list(&mut self) -> Option<&mut Files> {
        if self.detail_tab != DetailTab::Files {
            return None;
        }
        self.files
            .as_mut()
            .filter(|files| files.changes.is_some() && files.diff.is_none())
    }

    /// Moves the file selection; false when no file list is shown.
    pub(super) fn move_file_selection(&mut self, down: bool) -> bool {
        let Some(files) = self.shown_file_list() else {
            return false;
        };
        files.select(down);
        true
    }

    pub(super) fn open_file_diff(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.detail.clone() else {
            return;
        };
        let Some(api) = self.api() else {
            return;
        };
        let Some(files) = self.shown_file_list() else {
            return;
        };
        let (Some(changes), Some(file)) = (
            files.changes.clone(),
            files.list().get(files.selected).cloned(),
        ) else {
            return;
        };
        let (id, index) = (files.pr_id, files.selected);
        files.diff = Some((index, None));
        self.detail_scroll = 0;
        self.spawn_load(Load::Diff, async move {
            let result = api
                .file_versions(&pr, &changes, &file)
                .await
                .map(|(old, new)| FileDiff::of(old, new));
            Message::FileDiff {
                id,
                file: index,
                result,
            }
        });
    }

    /// Back from a diff to the file list; false when no diff is shown.
    pub(super) fn close_file_diff(&mut self) -> bool {
        if self.detail_tab != DetailTab::Files {
            return false;
        }
        let Some(files) = self.files.as_mut().filter(|files| files.diff.is_some()) else {
            return false;
        };
        files.diff = None;
        self.loads.cancel(Load::Diff);
        self.detail_scroll = 0;
        true
    }

    pub(super) fn files_loaded(&mut self, id: u32, result: Result<PullRequestChanges>) {
        if !self.files.as_ref().is_some_and(|files| files.pr_id == id) {
            return;
        }
        match result {
            Ok(changes) => {
                if let Some(files) = &mut self.files {
                    files.changes = Some(changes);
                }
            }
            Err(err) => {
                if let Some(files) = &mut self.files {
                    files.error = Some(err.to_string());
                }
                self.report_error(err);
            }
        }
    }

    pub(super) fn diff_loaded(&mut self, id: u32, file: usize, result: Result<FileDiff>) {
        let Some(files) = self.files.as_mut().filter(|files| {
            files.pr_id == id && files.diff.as_ref().is_some_and(|(open, _)| *open == file)
        }) else {
            return;
        };
        match result {
            Ok(diff) => files.diff = Some((file, Some(diff))),
            Err(err) => {
                files.diff = None;
                self.report_error(err);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use crossterm::event::KeyCode;

    use super::*;
    use crate::api::{AdoClient, ChangeKind};
    use crate::app::Panel;
    use crate::app::test_helpers::{app, ctrl, open_pull_request, press};
    use crate::auth::Auth;
    use crate::diff::{DiffLine, DiffLineKind};

    fn file(path: &str) -> ChangedFile {
        ChangedFile {
            kind: ChangeKind::Modified,
            path: path.into(),
            original_path: None,
        }
    }

    fn changes() -> PullRequestChanges {
        PullRequestChanges {
            base: "base".into(),
            head: "head".into(),
            files: vec![file("/a.rs"), file("/b.rs"), file("/c.rs")],
        }
    }

    /// The shown PR on its Files tab, with the detail focused and the files loaded.
    fn on_files_tab() -> App {
        let mut app = app();
        // Nothing listens there; diff requests stay pending or fail, and are never handled.
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:9",
        ));
        app.sync_api();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');
        app.detail_tab = DetailTab::Files;
        app.files = Some(Files::new(1));
        app.files_loaded(1, Ok(changes()));
        app
    }

    fn selected(app: &App) -> usize {
        app.files.as_ref().unwrap().selected
    }

    fn open_diff(app: &App) -> Option<usize> {
        app.files
            .as_ref()
            .unwrap()
            .diff
            .as_ref()
            .map(|(file, _)| *file)
    }

    #[tokio::test]
    async fn j_and_k_move_the_file_selection_within_the_list() {
        let mut app = on_files_tab();

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(selected(&app), 0);
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(selected(&app), 2);
        press(&mut app, KeyCode::Char('k'));

        assert_eq!(selected(&app), 1);
        assert_eq!(app.detail_scroll, 0);
    }

    #[tokio::test]
    async fn j_scrolls_instead_on_other_tabs_and_in_a_diff() {
        let mut app = on_files_tab();
        app.detail_tab = DetailTab::Checks;
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.detail_scroll, 1);

        app.detail_tab = DetailTab::Files;
        app.detail_scroll = 0;
        app.files.as_mut().unwrap().diff = Some((0, None));
        press(&mut app, KeyCode::Char('j'));

        assert_eq!(app.detail_scroll, 1);
        assert_eq!(selected(&app), 0);
    }

    #[tokio::test]
    async fn enter_opens_the_selected_file_and_esc_returns_to_the_list() {
        let mut app = on_files_tab();
        press(&mut app, KeyCode::Char('j'));

        press(&mut app, KeyCode::Enter);
        assert_eq!(open_diff(&app), Some(1));
        app.detail_scroll = 4;

        press(&mut app, KeyCode::Esc);

        assert_eq!(open_diff(&app), None);
        assert_eq!(app.detail_scroll, 0);
        assert!(app.detail.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(app.detail.is_none());
    }

    #[tokio::test]
    async fn enter_on_a_left_list_opens_the_item_instead() {
        let mut app = on_files_tab();
        app.focus = Panel::MyPrs;

        press(&mut app, KeyCode::Enter);

        assert!(app.files.is_none());
        assert_eq!(app.detail_tab, DetailTab::Overview);
    }

    #[tokio::test]
    async fn diff_for_another_file_or_pull_request_is_ignored() {
        let mut app = on_files_tab();
        app.files.as_mut().unwrap().diff = Some((1, None));
        let diff = || {
            Ok(FileDiff::Lines(vec![DiffLine {
                kind: DiffLineKind::Added,
                text: "x".into(),
            }]))
        };

        app.diff_loaded(1, 0, diff());
        app.diff_loaded(2, 1, diff());
        assert!(matches!(app.files.as_ref().unwrap().diff, Some((1, None))));

        app.diff_loaded(1, 1, diff());
        assert!(matches!(
            app.files.as_ref().unwrap().diff,
            Some((1, Some(FileDiff::Lines(_))))
        ));
    }

    #[tokio::test]
    async fn failed_diff_shows_the_error_and_the_list_again() {
        let mut app = on_files_tab();
        app.files.as_mut().unwrap().diff = Some((0, None));

        app.diff_loaded(1, 0, Err(anyhow!("boom")));

        assert_eq!(open_diff(&app), None);
        assert_eq!(app.error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn failed_file_list_is_loaded_again_next_time() {
        let mut app = on_files_tab();
        app.files = Some(Files::new(1));

        app.files_loaded(1, Err(anyhow!("boom")));

        let files = app.files.as_ref().unwrap();
        assert!(files.changes.is_none());
        assert_eq!(files.error.as_deref(), Some("boom"));
        assert_eq!(app.error.as_deref(), Some("boom"));

        app.load_files();

        assert!(app.files.as_ref().unwrap().error.is_none());
    }

    #[tokio::test]
    async fn files_of_another_pull_request_are_ignored() {
        let mut app = on_files_tab();
        app.files = Some(Files::new(1));

        app.files_loaded(2, Ok(changes()));

        assert!(app.files.as_ref().unwrap().changes.is_none());
    }

    #[tokio::test]
    async fn opening_another_item_drops_the_files() {
        let mut app = on_files_tab();
        app.focus = Panel::MyPrs;
        press(&mut app, KeyCode::Char('j'));

        press(&mut app, KeyCode::Enter);

        assert!(app.files.is_none());
    }
}
