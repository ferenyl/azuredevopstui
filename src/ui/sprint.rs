use chrono::{DateTime, Datelike, Local, NaiveDate};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::{field, heading, short_date};
use crate::app::App;

const MAX_BAR: usize = 30;

/// Shown in the detail panel when nothing is open.
pub fn overview(app: &App, width: u16) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    let data = &app.data;
    let Some(sprint) = &data.sprint_name else {
        return vec![Line::styled("Select an item and press enter", muted)];
    };
    let mut lines = vec![heading(app, sprint, width)];
    if let (Some(start), Some(finish)) = (&data.sprint_start, &data.sprint_finish) {
        let day = |date: &str| short_date(date).get(..10).unwrap_or_default().to_string();
        lines.push(field(
            app,
            "Dates",
            vec![Span::raw(format!("{} – {}", day(start), day(finish)))],
        ));
    }
    if let Some(days) = data.sprint_finish.as_deref().and_then(work_days_left) {
        lines.push(field(
            app,
            "Left",
            vec![Span::raw(format!("{days} work days"))],
        ));
    }

    let columns = my_columns(app);
    if !columns.is_empty() {
        lines.push(Line::default());
        lines.push(heading(app, "My items per column", width));
        let name_width = columns.iter().map(|(name, _)| name.chars().count()).max();
        let name_width = name_width.unwrap_or(0);
        for (name, count) in columns {
            lines.push(Line::from(vec![
                Span::raw(format!("  {name:<name_width$}  {count:>3}  ")),
                Span::styled(
                    "█".repeat(count.min(MAX_BAR)),
                    Style::new().fg(app.theme.border_focused),
                ),
            ]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::styled("Select an item and press enter", muted));
    lines
}

/// Weekdays from today through the finish date.
pub fn work_days_left(finish: &str) -> Option<usize> {
    let finish = DateTime::parse_from_rfc3339(finish).ok()?.date_naive();
    Some(work_days_between(Local::now().date_naive(), finish))
}

fn work_days_between(from: NaiveDate, to: NaiveDate) -> usize {
    from.iter_days()
        .take_while(|day| *day <= to)
        .filter(|day| day.weekday().number_from_monday() <= 5)
        .count()
}

/// My items counted per board column, in board order when the board is loaded.
fn my_columns(app: &App) -> Vec<(String, usize)> {
    let Some(items) = &app.data.my_work_items else {
        return Vec::new();
    };
    let mut columns: Vec<(String, usize)> = app
        .board_columns()
        .into_iter()
        .map(|name| (name, 0))
        .collect();
    for item in items {
        let column = item
            .fields
            .board_column
            .clone()
            .unwrap_or_else(|| item.fields.state.clone());
        match columns.iter_mut().find(|(name, _)| *name == column) {
            Some((_, count)) => *count += 1,
            None => columns.push((column, 1)),
        }
    }
    columns
}
