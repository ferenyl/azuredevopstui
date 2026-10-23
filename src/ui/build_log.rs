use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{empty, heading, loading, wrapped_rows};
use crate::api::TimelineRecord;
use crate::app::{App, DetailInfo, Panel};

/// The Timeline tab: the run's stages, jobs and tasks, or the open task log.
pub fn lines(app: &App, width: u16) -> Vec<Line<'static>> {
    if let Some(log) = &app.log {
        let heading = heading(app, &log.title, width);
        return match &log.lines {
            None => [vec![heading], loading(app)].concat(),
            Some(lines) if lines.is_empty() => [vec![heading], empty(app, "Empty log")].concat(),
            Some(lines) => std::iter::once(heading)
                .chain(lines.iter().map(|line| log_line(app, line)))
                .collect(),
        };
    }
    if !matches!(app.detail_info, Some(DetailInfo::Build(_))) {
        return loading(app);
    }
    if app.timeline().is_empty() {
        return empty(app, "No timeline");
    }
    timeline_lines(app).0
}

/// Scroll that keeps the selected task visible in `height` rows of `width` columns, while the
/// timeline is shown.
pub fn list_scroll(app: &App, height: u16, width: u16) -> Option<u16> {
    if app.log.is_some() || app.timeline().is_empty() {
        return None;
    }
    let (lines, selected) = timeline_lines(app);
    let rows: usize = lines
        .iter()
        .take(selected? + 1)
        .map(|line| wrapped_rows(line, width))
        .sum();
    let scroll = rows.saturating_sub(usize::from(height));
    Some(u16::try_from(scroll).unwrap_or(u16::MAX))
}

/// The timeline lines, with the index of the line of the selected task.
fn timeline_lines(app: &App) -> (Vec<Line<'static>>, Option<usize>) {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    let highlight = if app.focus == Panel::Detail {
        Style::new().bg(theme.selection_bg).fg(theme.selection_fg)
    } else {
        Style::new().bg(theme.selection_bg)
    };
    let selected = app.selected_timeline_row();
    let mut lines = Vec::new();
    let mut selected_line = None;
    for (index, row) in app.timeline().iter().enumerate() {
        let record = &row.record;
        let (symbol, color) = status(app, record);
        let mut spans = vec![
            Span::raw("  ".repeat(row.depth)),
            Span::styled(format!("{symbol} "), Style::new().fg(color)),
            Span::raw(record.name.clone()),
        ];
        if record.error_count > 0 {
            let plural = if record.error_count == 1 { "" } else { "s" };
            spans.push(Span::styled(
                format!("  ({} error{plural})", record.error_count),
                Style::new().fg(theme.build_failed),
            ));
        }
        let line = Line::from(spans);
        if selected == Some(index) {
            selected_line = Some(lines.len());
            lines.push(line.style(highlight));
        } else {
            lines.push(line);
        }
        if record.kind == "Task" && record.failed() {
            for issue in &record.issues {
                if let Some(message) = &issue.message {
                    lines.push(Line::styled(
                        format!("{}    {message}", "  ".repeat(row.depth)),
                        muted,
                    ));
                }
            }
        }
    }
    (lines, selected_line)
}

fn status(app: &App, record: &TimelineRecord) -> (&'static str, Color) {
    let theme = &app.theme;
    match (record.state.as_str(), record.result.as_deref()) {
        ("inProgress", _) => ("⟳", theme.build_running),
        ("completed", Some("succeeded")) => ("✔", theme.build_succeeded),
        ("completed", Some("succeededWithIssues")) => ("◑", theme.pr_waiting),
        ("completed", Some("failed")) => ("✖", theme.build_failed),
        ("completed", Some("canceled" | "abandoned" | "skipped")) => ("⊘", theme.muted),
        ("completed", _) => ("–", theme.muted),
        _ => ("○", theme.muted),
    }
}

fn log_line(app: &App, line: &str) -> Line<'static> {
    let theme = &app.theme;
    if line.contains("##[error]") {
        Line::styled(line.to_string(), Style::new().fg(theme.build_failed))
    } else if line.contains("##[warning]") {
        Line::styled(line.to_string(), Style::new().fg(theme.pr_waiting))
    } else {
        Line::raw(line.to_string())
    }
}
