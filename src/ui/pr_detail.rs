use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{heading, indented, short_date};
use crate::api::{PullRequest, PullRequestDetails};
use crate::app::App;

pub fn lines(
    app: &App,
    pr: &PullRequest,
    details: Option<&PullRequestDetails>,
) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    let mut status = pr.status.clone();
    if pr.is_draft {
        status.push_str(" [draft]");
    }
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Repo ", muted),
            Span::raw(pr.repository.name.clone()),
            Span::styled("   Branch ", muted),
            Span::raw(format!(
                "{} → {}",
                branch(&pr.source_ref_name),
                branch(&pr.target_ref_name)
            )),
        ]),
        Line::from(vec![
            Span::styled("Status ", muted),
            Span::raw(status),
            Span::styled("   Merge ", muted),
            Span::raw(pr.merge_status.clone().unwrap_or_else(|| "-".into())),
        ]),
        Line::from(vec![
            Span::styled("Created by ", muted),
            Span::raw(pr.created_by.display_name.clone()),
            Span::styled(format!(" · {}", short_date(&pr.creation_date)), muted),
        ]),
        Line::default(),
        heading(app, "Reviewers"),
    ];

    if pr.reviewers.is_empty() {
        lines.push(Line::styled("  none", muted));
    }
    for reviewer in &pr.reviewers {
        let (symbol, text, color) = vote(app, reviewer.vote);
        let mut spans = vec![
            Span::styled(format!("  {symbol} "), Style::new().fg(color)),
            Span::raw(reviewer.display_name.clone()),
        ];
        if reviewer.is_required == Some(true) {
            spans.push(Span::styled(" (required)", muted));
        }
        spans.push(Span::styled(format!("  {text}"), muted));
        lines.push(Line::from(spans));
    }

    if let Some(description) = pr.description.as_deref().filter(|d| !d.trim().is_empty()) {
        lines.push(Line::default());
        lines.push(heading(app, "Description"));
        lines.extend(indented(description, 2));
    }

    lines.push(Line::default());
    lines.push(heading(app, "Checks"));
    let Some(details) = details else {
        lines.push(Line::styled("  Loading…", muted));
        return lines;
    };
    if details.policies.is_empty() && details.statuses.is_empty() {
        lines.push(Line::styled("  none", muted));
    }
    for policy in &details.policies {
        let settings = &policy.configuration.settings;
        let name = settings
            .display_name
            .clone()
            .unwrap_or_else(|| policy.configuration.kind.display_name.clone());
        let (symbol, color) = check_state(app, &policy.status);
        let mut spans = vec![
            Span::styled(format!("  {symbol} "), Style::new().fg(color)),
            Span::raw(name),
        ];
        if !policy.configuration.is_blocking {
            spans.push(Span::styled(" (optional)", muted));
        }
        lines.push(Line::from(spans));
    }
    for status in &details.statuses {
        let state = status.state.as_deref().unwrap_or("pending");
        let (symbol, color) = check_state(app, state);
        let name = match &status.context.genre {
            Some(genre) => format!("{genre}/{}", status.context.name),
            None => status.context.name.clone(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {symbol} "), Style::new().fg(color)),
            Span::raw(name),
            Span::styled(
                format!("  {}", status.description.clone().unwrap_or_default()),
                muted,
            ),
        ]));
    }

    lines.push(Line::default());
    lines.push(heading(app, "Comments"));
    if details.threads.is_empty() {
        lines.push(Line::styled("  none", muted));
    }
    for thread in &details.threads {
        let file = thread
            .thread_context
            .as_ref()
            .and_then(|context| context.file_path.clone())
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                format!("  ● {}", thread.status.as_deref().unwrap_or("-")),
                Style::new().fg(theme.title),
            ),
            Span::styled(format!("  {file}"), muted),
        ]));
        for comment in &thread.comments {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    {}", comment.author.display_name),
                    Style::new().fg(theme.comment_author),
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
            lines.extend(indented(comment.content.as_deref().unwrap_or_default(), 6));
        }
    }
    lines
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

fn check_state(app: &App, state: &str) -> (&'static str, Color) {
    let theme = &app.theme;
    match state {
        "approved" | "succeeded" => ("✔", theme.build_succeeded),
        "rejected" | "broken" | "failed" | "error" => ("✖", theme.build_failed),
        "notApplicable" => ("–", theme.muted),
        _ => ("⟳", theme.build_running),
    }
}
