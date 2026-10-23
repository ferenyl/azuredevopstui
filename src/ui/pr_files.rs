use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{empty, heading, loading};
use crate::api::{ChangeKind, ChangedFile};
use crate::app::{App, Files, Panel};
use crate::diff::{DiffLineKind, FileDiff};

pub fn lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let Some(files) = app.files.as_ref().filter(|files| files.changes.is_some()) else {
        if let Some(error) = app.files.as_ref().and_then(|files| files.error.as_ref()) {
            return empty(app, &format!("Could not load changes: {error}"));
        }
        return loading(app);
    };
    match &files.diff {
        Some((index, diff)) => match files.list().get(*index) {
            Some(file) => diff_lines(app, file, diff.as_ref(), width),
            None => loading(app),
        },
        None if files.list().is_empty() => empty(app, "No changed files"),
        None => file_lines(app, files),
    }
}

/// Scroll that keeps the selected file visible in `height` rows of `width` columns, while the
/// list is shown. Long paths wrap onto several rows.
pub fn list_scroll(app: &App, height: u16, width: u16) -> Option<u16> {
    let files = app.files.as_ref().filter(|files| files.diff.is_none())?;
    let width = usize::from(width).max(1);
    let rows: usize = files
        .list()
        .iter()
        .take(files.selected + 1)
        .map(|file| {
            (super::text_width(file.kind.label()) + 1 + super::text_width(&path_text(file)))
                .div_ceil(width)
                .max(1)
        })
        .sum();
    let scroll = rows.saturating_sub(usize::from(height));
    Some(u16::try_from(scroll).unwrap_or(u16::MAX))
}

fn path_text(file: &ChangedFile) -> String {
    match &file.original_path {
        Some(original) => format!("{original} → {}", file.path),
        None => file.path.clone(),
    }
}

fn file_lines(app: &App, files: &Files) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let highlight = if app.focus == Panel::Detail {
        Style::new().bg(theme.selection_bg).fg(theme.selection_fg)
    } else {
        Style::new().bg(theme.selection_bg)
    };
    files
        .list()
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let path = path_text(file);
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", file.kind.label()),
                    Style::new().fg(kind_color(app, file.kind)),
                ),
                Span::raw(path),
            ]);
            if index == files.selected {
                line.style(highlight)
            } else {
                line
            }
        })
        .collect()
}

fn diff_lines(
    app: &App,
    file: &ChangedFile,
    diff: Option<&FileDiff>,
    width: u16,
) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let mut lines = vec![heading(app, &file.path, width)];
    let diff = match diff {
        None => return [lines, loading(app)].concat(),
        Some(FileDiff::Skipped) => {
            return [lines, empty(app, "Binary or too large to show")].concat();
        }
        Some(FileDiff::Lines(diff)) if diff.is_empty() => {
            return [lines, empty(app, "No content changes")].concat();
        }
        Some(FileDiff::Lines(diff)) => diff,
    };
    lines.extend(diff.iter().map(|line| {
        let (sign, color) = match line.kind {
            DiffLineKind::Hunk => ("", theme.muted),
            DiffLineKind::Context => (" ", theme.foreground),
            DiffLineKind::Added => ("+", theme.build_succeeded),
            DiffLineKind::Removed => ("-", theme.build_failed),
        };
        Line::styled(format!("{sign}{}", line.text), Style::new().fg(color))
    }));
    lines
}

fn kind_color(app: &App, kind: ChangeKind) -> Color {
    let theme = &app.theme;
    match kind {
        ChangeKind::Added => theme.build_succeeded,
        ChangeKind::Deleted => theme.build_failed,
        ChangeKind::Renamed => theme.title,
        ChangeKind::Modified => theme.build_running,
    }
}
