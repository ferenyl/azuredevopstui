use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::{badge, empty, field, heading, loading, short_date, wrap};
use crate::api::{PullRequest, PullRequestDetails};
use crate::app::{App, DetailTab};

pub fn lines(
    app: &App,
    pr: &PullRequest,
    details: Option<&PullRequestDetails>,
    tab: DetailTab,
    width: u16,
) -> Vec<Line<'static>> {
    match (tab, details) {
        (DetailTab::Overview, _) => overview(app, pr, width),
        (_, None) => loading(app),
        (DetailTab::Comments, Some(details)) => comments(app, details, width),
        (DetailTab::Checks, Some(details)) => checks(app, details, width),
    }
}

fn overview(app: &App, pr: &PullRequest, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    let status_color = match pr.status.as_str() {
        "completed" => theme.pr_approved,
        "abandoned" => theme.muted,
        _ => theme.build_running,
    };
    let mut status = vec![badge(&pr.status, status_color)];
    if pr.is_draft {
        status.push(Span::styled("  draft", Style::new().fg(theme.pr_waiting)));
    }
    let merge = pr.merge_status.clone().unwrap_or_else(|| "-".into());
    let merge_color = match merge.as_str() {
        "succeeded" => theme.pr_approved,
        "conflicts" | "failure" | "rejectedByPolicy" => theme.error,
        _ => theme.foreground,
    };

    let mut lines = vec![
        field(app, "Status", status),
        field(app, "Repo", vec![Span::raw(pr.repository.name.clone())]),
        field(
            app,
            "Branch",
            vec![
                Span::raw(branch(&pr.source_ref_name).to_string()),
                Span::styled(" → ", muted),
                Span::raw(branch(&pr.target_ref_name).to_string()),
            ],
        ),
        field(
            app,
            "Merge",
            vec![Span::styled(merge, Style::new().fg(merge_color))],
        ),
        field(
            app,
            "Author",
            vec![Span::raw(pr.created_by.display_name.clone())],
        ),
        field(
            app,
            "Created",
            vec![Span::raw(short_date(&pr.creation_date))],
        ),
        Line::default(),
        heading(app, "Reviewers", width),
    ];

    if pr.reviewers.is_empty() {
        lines.push(Line::styled("  none", muted));
    }
    let name_width = pr
        .reviewers
        .iter()
        .map(|r| r.display_name.chars().count())
        .max()
        .unwrap_or(0);
    for reviewer in &pr.reviewers {
        let (symbol, text, color) = vote(app, reviewer.vote);
        let mut spans = vec![
            Span::styled(format!("  {symbol} "), Style::new().fg(color)),
            Span::raw(format!("{:<name_width$}  ", reviewer.display_name)),
            Span::styled(text, Style::new().fg(color)),
        ];
        if reviewer.is_required == Some(true) {
            spans.push(Span::styled("  required", muted));
        }
        lines.push(Line::from(spans));
    }

    if let Some(description) = pr.description.as_deref().filter(|d| !d.trim().is_empty()) {
        lines.push(Line::default());
        lines.push(heading(app, "Description", width));
        lines.extend(wrap(description, width, Span::raw("  ")));
    }
    lines
}

fn comments(app: &App, details: &PullRequestDetails, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    if details.threads.is_empty() {
        return empty(app, "No comments");
    }
    let bar = Span::styled("  │ ", Style::new().fg(theme.border));
    let mut lines = Vec::new();
    for thread in &details.threads {
        let status = thread.status.as_deref().unwrap_or("-");
        let color = match status {
            "active" | "pending" => theme.pr_waiting,
            "fixed" | "closed" | "byDesign" => theme.pr_approved,
            _ => theme.muted,
        };
        let file = thread
            .thread_context
            .as_ref()
            .and_then(|context| context.file_path.clone())
            .unwrap_or_default();
        lines.push(Line::from(vec![
            badge(status, color),
            Span::styled(format!("  {file}"), muted),
        ]));
        for (index, comment) in thread.comments.iter().enumerate() {
            if index > 0 {
                lines.push(Line::from(bar.clone()));
            }
            lines.push(Line::from(vec![
                bar.clone(),
                Span::styled(
                    comment.author.display_name.clone(),
                    Style::new()
                        .fg(theme.comment_author)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        " · {}",
                        comment
                            .published_date
                            .as_deref()
                            .map(short_date)
                            .unwrap_or_default()
                    ),
                    muted,
                ),
            ]));
            lines.extend(wrap(
                comment.content.as_deref().unwrap_or_default(),
                width,
                bar.clone(),
            ));
        }
        lines.push(Line::default());
    }
    lines
}

fn checks(app: &App, details: &PullRequestDetails, width: u16) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    if details.policies.is_empty() && details.statuses.is_empty() {
        return empty(app, "No checks");
    }
    let states: Vec<CheckState> = details
        .policies
        .iter()
        .map(|policy| CheckState::from(policy.status.as_str()))
        .chain(
            details
                .statuses
                .iter()
                .map(|status| CheckState::from(status.state.as_deref().unwrap_or("pending"))),
        )
        .collect();
    let mut summary = Vec::new();
    for (state, label) in [
        (CheckState::Passed, "passed"),
        (CheckState::Failed, "failed"),
        (CheckState::Pending, "pending"),
    ] {
        let count = states.iter().filter(|s| **s == state).count();
        if count > 0 {
            if !summary.is_empty() {
                summary.push(Span::styled("   ", muted));
            }
            summary.push(Span::styled(
                format!("{} {count} {label}", state.symbol()),
                Style::new()
                    .fg(state.color(app))
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    let mut lines = vec![Line::from(summary)];

    if !details.policies.is_empty() {
        lines.push(Line::default());
        lines.push(heading(app, "Policies", width));
    }
    for policy in &details.policies {
        let settings = &policy.configuration.settings;
        let name = settings
            .display_name
            .clone()
            .unwrap_or_else(|| policy.configuration.kind.display_name.clone());
        let mut spans = check_spans(app, CheckState::from(policy.status.as_str()), name);
        if !policy.configuration.is_blocking {
            spans.push(Span::styled("  optional", muted));
        }
        lines.push(Line::from(spans));
    }

    if !details.statuses.is_empty() {
        lines.push(Line::default());
        lines.push(heading(app, "Statuses", width));
    }
    for status in &details.statuses {
        let state = CheckState::from(status.state.as_deref().unwrap_or("pending"));
        let name = match &status.context.genre {
            Some(genre) => format!("{genre}/{}", status.context.name),
            None => status.context.name.clone(),
        };
        let mut spans = check_spans(app, state, name);
        if let Some(description) = status.description.clone().filter(|d| !d.is_empty()) {
            spans.push(Span::styled(format!("  {description}"), muted));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn check_spans(app: &App, state: CheckState, name: String) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("  {} ", state.symbol()),
            Style::new().fg(state.color(app)),
        ),
        Span::raw(name),
    ]
}

fn branch(ref_name: &str) -> &str {
    ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name)
}

fn vote(app: &App, vote: i32) -> (&'static str, &'static str, Color) {
    let theme = &app.theme;
    match vote {
        10 => ("✔", "approved", theme.pr_approved),
        5 => ("✔", "approved with suggestions", theme.pr_approved),
        -5 => ("⏳", "waiting for author", theme.pr_waiting),
        -10 => ("✖", "rejected", theme.pr_rejected),
        _ => ("○", "no vote", theme.muted),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckState {
    Passed,
    Failed,
    NotApplicable,
    Pending,
}

impl CheckState {
    fn from(state: &str) -> Self {
        match state {
            "approved" | "succeeded" => Self::Passed,
            "rejected" | "broken" | "failed" | "error" => Self::Failed,
            "notApplicable" => Self::NotApplicable,
            _ => Self::Pending,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Passed => "✔",
            Self::Failed => "✖",
            Self::NotApplicable => "–",
            Self::Pending => "⟳",
        }
    }

    fn color(self, app: &App) -> Color {
        let theme = &app.theme;
        match self {
            Self::Passed => theme.build_succeeded,
            Self::Failed => theme.build_failed,
            Self::NotApplicable => theme.muted,
            Self::Pending => theme.build_running,
        }
    }
}
