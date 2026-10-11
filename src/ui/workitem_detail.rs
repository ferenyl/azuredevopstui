use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::{
    badge, empty, field, heading, loading, rich_lines, short_date, state_color, type_color,
};
use crate::api::{Link, LinkTarget, PullRequest, WorkItem, WorkItemDetails};
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
        DetailTab::Links => {
            details.map_or_else(|| loading(app), |details| links(app, details, width))
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
    lines.extend(
        details
            .pull_requests
            .iter()
            .map(|pr| pull_request_line(app, pr)),
    );

    for (title, text) in [
        ("Description", &details.description),
        ("Repro steps", &details.repro_steps),
        ("Acceptance criteria", &details.acceptance_criteria),
    ] {
        if let Some(text) = text.as_ref().filter(|t| !t.is_empty()) {
            lines.push(Line::default());
            lines.push(heading(app, title, width));
            lines.extend(rich_lines(app, text, width, Span::raw("  ")));
        }
    }
    lines
}

fn children(app: &App, details: &WorkItemDetails, width: u16) -> Vec<Line<'static>> {
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
        lines.extend(children.into_iter().map(|child| work_item_line(app, child)));
    }
    lines
}

fn links(app: &App, details: &WorkItemDetails, width: u16) -> Vec<Line<'static>> {
    if details.links.is_empty() && details.pull_requests.is_empty() {
        return empty(app, "No links");
    }
    let (work_items, others): (Vec<&Link>, Vec<&Link>) = details
        .links
        .iter()
        .partition(|link| matches!(link.target, LinkTarget::WorkItem(_)));
    let mut lines = Vec::new();
    link_groups(app, &work_items, width, &mut lines);
    if !details.pull_requests.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let title = format!("Pull requests ({})", details.pull_requests.len());
        lines.push(heading(app, &title, width));
        lines.extend(
            details
                .pull_requests
                .iter()
                .map(|pr| pull_request_line(app, pr)),
        );
    }
    link_groups(app, &others, width, &mut lines);
    lines
}

fn link_groups(app: &App, links: &[&Link], width: u16, lines: &mut Vec<Line<'static>>) {
    for group in links.chunk_by(|a, b| a.category == b.category) {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let title = format!("{} ({})", group[0].category, group.len());
        lines.push(heading(app, &title, width));
        lines.extend(group.iter().map(|link| match &link.target {
            LinkTarget::WorkItem(item) => work_item_line(app, item),
            LinkTarget::Text(text) => Line::raw(format!("  {text}")),
        }));
    }
}

fn work_item_line(app: &App, item: &WorkItem) -> Line<'static> {
    let fields = &item.fields;
    let mut spans = vec![
        Span::styled(
            format!("  #{} ", item.id),
            Style::new().fg(type_color(app, &fields.work_item_type)),
        ),
        Span::raw(fields.title.clone()),
        Span::raw("  "),
        badge(&fields.state, state_color(app, &fields.state)),
    ];
    if let Some(assignee) = &fields.assigned_to {
        spans.push(Span::styled(
            format!(" · {}", assignee.display_name),
            Style::new().fg(app.theme.muted),
        ));
    }
    Line::from(spans)
}

fn pull_request_line(app: &App, pr: &PullRequest) -> Line<'static> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
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
    Line::from(spans)
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
        lines.extend(rich_lines(app, &comment.text, width, bar.clone()));
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
