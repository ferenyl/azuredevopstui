use anyhow::Result;

use crate::api::{TimelineRecord, TimelineRow, timeline_rows};

use super::input::Move;
use super::loads::Load;
use super::message::Message;
use super::{App, Detail, DetailInfo, DetailTab};

/// A task log of the shown pipeline run, open in the detail pane.
pub struct BuildLog {
    pub build_id: u32,
    pub log_id: u32,
    pub title: String,
    /// `None` while loading.
    pub lines: Option<Vec<String>>,
}

impl App {
    /// The timeline of the shown pipeline run; empty while loading.
    pub fn timeline(&self) -> &[TimelineRow] {
        match &self.detail_info {
            Some(DetailInfo::Build(rows)) => rows,
            _ => &[],
        }
    }

    /// Indexes of the timeline rows that have a log, which the selection moves over.
    pub fn log_rows(&self) -> Vec<usize> {
        self.timeline()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.record.log.is_some())
            .map(|(index, _)| index)
            .collect()
    }

    /// The timeline row the selection is on.
    pub fn selected_timeline_row(&self) -> Option<usize> {
        self.log_rows().get(self.timeline_selection).copied()
    }

    pub(super) fn timeline_loaded(&mut self, id: u32, result: Result<Vec<TimelineRecord>>) {
        if !self.detail.as_ref().is_some_and(|d| d.is_build(id)) {
            return;
        }
        match result {
            Ok(records) => {
                let first = self.detail_info.is_none();
                self.detail_info = Some(DetailInfo::Build(timeline_rows(&records)));
                let rows = self.log_rows();
                self.timeline_selection = if first {
                    rows.iter()
                        .position(|row| {
                            let record = &self.timeline()[*row].record;
                            record.kind == "Task" && record.failed()
                        })
                        .unwrap_or(0)
                } else {
                    self.timeline_selection.min(rows.len().saturating_sub(1))
                };
            }
            Err(err) => self.report_error(err),
        }
    }

    /// Moves the selection over the tasks with a log, `page` rows being visible; false when no
    /// timeline is shown.
    pub(super) fn jump_timeline_selection(&mut self, step: Move, page: usize) -> bool {
        if self.detail_tab != DetailTab::Timeline
            || self.log.is_some()
            || !matches!(self.detail_info, Some(DetailInfo::Build(_)))
        {
            return false;
        }
        self.timeline_selection = step.target(self.timeline_selection, self.log_rows().len(), page);
        true
    }

    pub(super) fn open_build_log(&mut self) {
        if self.detail_tab != DetailTab::Timeline || self.log.is_some() {
            return;
        }
        let Some(Detail::Build(build)) = &self.detail else {
            return;
        };
        let Some(api) = self.api() else {
            return;
        };
        let Some(record) = self
            .selected_timeline_row()
            .map(|row| &self.timeline()[row].record)
        else {
            return;
        };
        let Some(log) = &record.log else {
            return;
        };
        let (build_id, log_id) = (build.id, log.id);
        self.log = Some(BuildLog {
            build_id,
            log_id,
            title: format!("{} › {}", build.definition.name, record.name),
            lines: None,
        });
        self.detail_scroll = 0;
        self.spawn_load(Load::Log, async move {
            let result = api.build_log(build_id, log_id).await;
            Message::BuildLog {
                build_id,
                log_id,
                result,
            }
        });
    }

    /// Back from a log to the timeline; false when no log is open.
    pub(super) fn close_build_log(&mut self) -> bool {
        if self.log.take().is_none() {
            return false;
        }
        self.loads.cancel(Load::Log);
        self.detail_scroll = 0;
        true
    }

    pub(super) fn clear_build_log(&mut self) {
        self.close_build_log();
        self.timeline_selection = 0;
    }

    pub(super) fn build_log_loaded(&mut self, build_id: u32, log_id: u32, result: Result<String>) {
        let Some(log) = self
            .log
            .as_mut()
            .filter(|log| log.build_id == build_id && log.log_id == log_id)
        else {
            return;
        };
        match result {
            Ok(text) => log.lines = Some(text.lines().map(clean_line).collect()),
            Err(err) => {
                self.log = None;
                self.report_error(err);
            }
        }
    }
}

/// A log line as shown: without the timestamp the agent puts in front of it, without colors,
/// progress rewrites and other control characters the terminal would act on.
fn clean_line(line: &str) -> String {
    let line = strip_timestamp(line);
    let line = line
        .rsplit('\r')
        .find(|part| !part.is_empty())
        .unwrap_or("");
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                if chars.next() == Some('[') {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
            }
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// `line` without the ISO timestamp the agent puts in front of it.
fn strip_timestamp(line: &str) -> &str {
    let line = line.trim_start_matches('\u{feff}');
    match line.split_once(' ') {
        Some((stamp, rest)) if is_timestamp(stamp) => rest,
        _ => line,
    }
}

/// `2026-10-01T10:00:00.1234567Z`
fn is_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 20
        && bytes.ends_with(b"Z")
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use crossterm::event::KeyCode;
    use serde_json::json;

    use super::*;
    use crate::api::AdoClient;
    use crate::app::Panel;
    use crate::app::test_helpers::{app, ctrl, press};
    use crate::auth::Auth;

    fn failed_build(id: u32) -> crate::api::Build {
        serde_json::from_value(json!({
            "id": id,
            "buildNumber": "1",
            "status": "completed",
            "result": "failed",
            "definition": { "id": 1, "name": "ci" },
            "sourceBranch": "refs/heads/main"
        }))
        .unwrap()
    }

    fn record(
        id: &str,
        parent: Option<&str>,
        kind: &str,
        result: &str,
        log: u32,
    ) -> TimelineRecord {
        serde_json::from_value(json!({
            "id": id,
            "parentId": parent,
            "type": kind,
            "name": id,
            "state": "completed",
            "result": result,
            "order": 1,
            "log": { "id": log }
        }))
        .unwrap()
    }

    fn timeline() -> Vec<TimelineRecord> {
        vec![
            record("stage", None, "Stage", "failed", 1),
            record("job", Some("stage"), "Job", "failed", 2),
            record("ok", Some("job"), "Task", "succeeded", 3),
            record("bad", Some("job"), "Task", "failed", 4),
            record("after", Some("job"), "Task", "succeeded", 5),
        ]
    }

    /// The shown pipeline run on its Timeline tab, with the detail focused.
    fn on_timeline() -> App {
        let mut app = app();
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:1",
        ));
        app.sync_api();
        app.detail = Some(Detail::Build(Box::new(failed_build(9))));
        app.detail_tab = DetailTab::Timeline;
        app.focus = Panel::Detail;
        app.timeline_loaded(9, Ok(timeline()));
        app
    }

    #[test]
    fn timestamps_are_stripped_from_log_lines() {
        assert_eq!(
            strip_timestamp("2026-10-01T10:00:00.1234567Z ##[error]boom"),
            "##[error]boom"
        );
        assert_eq!(
            strip_timestamp("\u{feff}2026-10-01T10:00:00.1234567Z hi"),
            "hi"
        );
        assert_eq!(strip_timestamp("no stamp here"), "no stamp here");
        assert_eq!(
            strip_timestamp("2026-10-01 not a stamp"),
            "2026-10-01 not a stamp"
        );
        assert_eq!(strip_timestamp(""), "");
    }

    #[test]
    fn control_characters_are_removed_from_log_lines() {
        assert_eq!(clean_line("\u{1b}[31;1mred\u{1b}[0m text"), "red text");
        assert_eq!(clean_line("10%\r50%\r100%"), "100%");
        assert_eq!(clean_line("a\tb\u{7}"), "a    b");
    }

    #[test]
    fn the_first_failed_task_is_preselected() {
        let app = on_timeline();

        let row = app.selected_timeline_row().unwrap();

        assert_eq!(app.timeline()[row].record.id, "bad");
    }

    #[test]
    fn reloading_keeps_the_selection() {
        let mut app = on_timeline();
        press(&mut app, KeyCode::Char('j'));

        app.timeline_loaded(9, Ok(timeline()));

        let row = app.selected_timeline_row().unwrap();
        assert_eq!(app.timeline()[row].record.id, "after");
    }

    #[test]
    fn a_timeline_of_another_run_is_ignored() {
        let mut app = on_timeline();

        app.timeline_loaded(8, Ok(Vec::new()));

        assert!(!app.timeline().is_empty());
    }

    #[test]
    fn j_and_k_move_over_the_rows_with_a_log_and_stop_at_the_ends() {
        let mut app = on_timeline();

        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.timeline_selection, 0);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.timeline_selection, 4);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.timeline_selection, 4);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.timeline_selection, 0);
        assert_eq!(app.detail_scroll, 0);
    }

    #[tokio::test]
    async fn enter_opens_the_log_of_the_selected_task_and_esc_goes_back() {
        let mut app = on_timeline();

        press(&mut app, KeyCode::Enter);

        let log = app.log.as_ref().unwrap();
        assert_eq!((log.build_id, log.log_id), (9, 4));
        assert_eq!(log.title, "ci › bad");
        assert!(log.lines.is_none());
        assert!(app.loads.is_loading(Load::Log));

        app.build_log_loaded(9, 4, Ok("2026-10-01T10:00:00.1Z one\ntwo".into()));
        assert_eq!(
            app.log.as_ref().unwrap().lines.as_deref().unwrap(),
            ["one", "two"]
        );

        press(&mut app, KeyCode::Esc);
        assert!(app.log.is_none());
        assert!(app.detail.is_some());
    }

    #[tokio::test]
    async fn keys_scroll_an_open_log_instead_of_moving_the_selection() {
        let mut app = on_timeline();
        press(&mut app, KeyCode::Enter);
        app.detail_max_scroll.set(10);

        press(&mut app, KeyCode::Char('j'));

        assert_eq!(app.detail_scroll, 1);
        assert_eq!(app.timeline_selection, 3);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.detail_scroll, 10);
    }

    #[tokio::test]
    async fn a_log_of_another_task_is_ignored() {
        let mut app = on_timeline();
        press(&mut app, KeyCode::Enter);

        app.build_log_loaded(9, 5, Ok("other".into()));
        app.build_log_loaded(8, 4, Ok("other".into()));

        assert!(app.log.as_ref().unwrap().lines.is_none());
    }

    #[tokio::test]
    async fn a_failed_log_load_closes_the_log_and_reports() {
        let mut app = on_timeline();
        press(&mut app, KeyCode::Enter);

        app.build_log_loaded(9, 4, Err(anyhow!("gone")));

        assert!(app.log.is_none());
        assert!(app.error.is_some());
    }

    #[test]
    fn enter_does_nothing_on_the_overview() {
        let mut app = on_timeline();
        app.detail_tab = DetailTab::Overview;

        press(&mut app, KeyCode::Enter);

        assert!(app.log.is_none());
    }

    #[tokio::test]
    async fn leaving_the_tab_closes_the_log() {
        let mut app = on_timeline();
        press(&mut app, KeyCode::Enter);

        ctrl(&mut app, 'h');
        press(&mut app, KeyCode::Tab);

        assert!(app.log.is_none());
        assert!(!app.loads.is_loading(Load::Log));
    }
}
