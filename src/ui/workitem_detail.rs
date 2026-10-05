use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::{badge, empty, field, heading, loading, short_date, state_color, type_color, wrap};
use crate::api::{WorkItem, WorkItemDetails};
use crate::app::{App, DetailTab};

pub fn lines(
    app: &App,
    item: &WorkItem,
    details: Option<&WorkItemDetails>,
    tab: DetailTab,
    width: u16,
) -> Vec<Line<'static>> {
    match tab {
        DetailTab::Overview => overview(app, item, details, width),
        DetailTab::Children => {
            details.map_or_else(|| loading(app), |details| children(app, details, width))
        }
        _ => details.map_or_else(|| loading(app), |details| comments(app, details, width)),
    }
}

fn overview(
    app: &App,
    item: &WorkItem,
    details: Option<&WorkItemDetails>,
    width: u16,
) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    let fields = &item.fields;
    let state = details.map_or(&fields.state, |d| &d.state);
    let mut lines = vec![
        field(
            app,
            "Type",
            vec![Span::styled(
                fields.work_item_type.clone(),
                Style::new()
                    .fg(type_color(app, &fields.work_item_type))
                    .add_modifier(Modifier::BOLD),
            )],
        ),
        field(app, "State", vec![badge(state, state_color(app, state))]),
    ];
    let Some(details) = details else {
        lines.push(Line::default());
        lines.extend(loading(app));
        return lines;
    };

    if let Some(column) = &details.board_column {
        let mut value = vec![Span::raw(column.clone())];
        if details.board_column_done {
            value.push(Span::styled(" · Done", muted));
        }
        lines.push(field(app, "Column", value));
    }
    if let Some(priority) = fields.priority {
        lines.push(field(
            app,
            "Priority",
            vec![Span::raw(priority.to_string())],
        ));
    }
    lines.push(field(
        app,
        "Assigned to",
        vec![match &details.assigned_to {
            Some(name) => Span::raw(name.clone()),
            None => Span::styled("unassigned", muted),
        }],
    ));
    if let Some(iteration) = &details.iteration_path {
        lines.push(field(app, "Sprint", vec![Span::raw(iteration.clone())]));
    }
    if let Some(tags) = details.tags.as_deref().filter(|t| !t.is_empty()) {
        let tags = tags
            .split(';')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .flat_map(|tag| {
                [
                    Span::styled(
                        format!(" {tag} "),
                        Style::new()
                            .fg(app.theme.selection_fg)
                            .bg(app.theme.selection_bg),
                    ),
                    Span::raw(" "),
                ]
            })
            .collect();
        lines.push(field(app, "Tags", tags));
    }

    if !details.pull_requests.is_empty() {
        lines.push(Line::default());
        lines.push(heading(app, "Pull requests", width));
    }
    for pr in &details.pull_requests {
        let theme = &app.theme;
        let color = match pr.status.as_str() {
            "completed" => theme.pr_approved,
            "abandoned" => theme.muted,
            _ => theme.build_running,
        };
        let mut spans = vec![
            Span::styled(format!("  !{} ", pr.pull_request_id), muted),
            Span::raw(pr.title.clone()),
            Span::raw("  "),
            badge(&pr.status, color),
        ];
        if pr.is_draft {
            spans.push(Span::styled(" · draft", muted));
        }
        spans.push(Span::styled(format!(" · {}", pr.repository.name), muted));
        lines.push(Line::from(spans));
    }

    for (title, text) in [
        ("Description", &details.description),
        ("Repro steps", &details.repro_steps),
        ("Acceptance criteria", &details.acceptance_criteria),
    ] {
        if let Some(text) = text.as_deref().filter(|t| !t.is_empty()) {
            lines.push(Line::default());
            lines.push(heading(app, title, width));
            lines.extend(wrap(text, width, Span::raw("  ")));
        }
    }
    lines
}

fn children(app: &App, details: &WorkItemDetails, width: u16) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    if details.children.is_empty() {
        return empty(app, "No children");
    }
    let mut types: Vec<&str> = details
        .children
        .iter()
        .map(|child| child.fields.work_item_type.as_str())
        .collect();
    types.sort();
    types.dedup();
    let mut lines = Vec::new();
    for kind in types {
        let children: Vec<_> = details
            .children
            .iter()
            .filter(|child| child.fields.work_item_type == kind)
            .collect();
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(heading(app, &format!("{kind} ({})", children.len()), width));
        for child in children {
            let fields = &child.fields;
            let mut spans = vec![
                Span::styled(
                    format!("  #{} ", child.id),
                    Style::new().fg(type_color(app, kind)),
                ),
                Span::raw(fields.title.clone()),
                Span::raw("  "),
                badge(&fields.state, state_color(app, &fields.state)),
            ];
            if let Some(assignee) = &fields.assigned_to {
                spans.push(Span::styled(format!(" · {}", assignee.display_name), muted));
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn comments(app: &App, details: &WorkItemDetails, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    if details.comments.is_empty() {
        return empty(app, "No comments");
    }
    let bar = Span::styled("│ ", Style::new().fg(theme.border));
    let mut lines = Vec::new();
    for comment in &details.comments {
        lines.push(Line::from(vec![
            Span::styled(
                comment.author.clone(),
                Style::new()
                    .fg(theme.comment_author)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {}", short_date(&comment.date)), muted),
        ]));
        lines.extend(wrap(&comment.text, width, bar.clone()));
        lines.push(Line::default());
    }
    let shown = details.comments.len();
    if (details.comment_count as usize) > shown {
        lines.push(Line::styled(
            format!(
                "Showing the {shown} newest of {} comments",
                details.comment_count
            ),
            muted,
        ));
    }
    lines
}
