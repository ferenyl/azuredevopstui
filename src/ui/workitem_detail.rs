use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::{heading, indented, short_date};
use crate::api::{WorkItem, WorkItemDetails};
use crate::app::App;

pub fn lines(app: &App, item: &WorkItem, details: Option<&WorkItemDetails>) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    let field = |label: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{label} "), muted),
            Span::raw(value),
        ])
    };
    let mut lines = vec![field(
        "Type",
        format!(
            "{}   State {}",
            item.fields.work_item_type, item.fields.state
        ),
    )];
    let Some(details) = details else {
        lines.push(Line::styled("Loading…", muted));
        return lines;
    };

    if let Some(column) = &details.board_column {
        let done = if details.board_column_done {
            " · Done"
        } else {
            ""
        };
        lines.push(field("Column", format!("{column}{done}")));
    }
    lines.push(field(
        "Assigned to",
        details.assigned_to.clone().unwrap_or_else(|| "-".into()),
    ));
    if let Some(iteration) = &details.iteration_path {
        lines.push(field("Sprint", iteration.clone()));
    }
    if let Some(tags) = &details.tags {
        lines.push(field("Tags", tags.clone()));
    }

    for (title, text) in [
        ("Description", &details.description),
        ("Repro steps", &details.repro_steps),
        ("Acceptance criteria", &details.acceptance_criteria),
    ] {
        if let Some(text) = text.as_deref().filter(|t| !t.is_empty()) {
            lines.push(Line::default());
            lines.push(heading(app, title));
            lines.extend(indented(text, 2));
        }
    }

    lines.push(Line::default());
    lines.push(heading(
        app,
        &format!("Comments ({})", details.comment_count),
    ));
    if details.comments.is_empty() {
        lines.push(Line::styled("  none", muted));
    }
    for comment in &details.comments {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}", comment.author),
                Style::new().fg(app.theme.comment_author),
            ),
            Span::styled(format!(" · {}", short_date(&comment.date)), muted),
        ]));
        lines.extend(indented(&comment.text, 4));
    }
    lines
}
