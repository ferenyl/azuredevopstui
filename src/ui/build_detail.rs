use chrono::DateTime;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{badge, field, short_date};
use crate::api::Build;
use crate::app::App;

pub fn lines(app: &App, build: &Build) -> Vec<Line<'static>> {
    let (_, label, color) = status(app, build);
    let mut lines = vec![
        field(
            app,
            "Pipeline",
            vec![Span::raw(build.definition.name.clone())],
        ),
        field(app, "Run", vec![Span::raw(build.build_number.clone())]),
        field(app, "Branch", vec![Span::raw(branch(build).to_string())]),
        field(app, "Status", vec![badge(label, color)]),
    ];
    if let Some(reason) = &build.reason {
        lines.push(field(app, "Reason", vec![Span::raw(reason.clone())]));
    }
    for (label, date) in [
        ("Queued", &build.queue_time),
        ("Started", &build.start_time),
        ("Finished", &build.finish_time),
    ] {
        if let Some(date) = date {
            lines.push(field(app, label, vec![Span::raw(short_date(date))]));
        }
    }
    if let Some(duration) = duration(build) {
        lines.push(field(app, "Duration", vec![Span::raw(duration)]));
    }
    lines
}

/// The list line: status symbol, pipeline and branch.
pub fn line(app: &App, build: &Build) -> Line<'static> {
    let muted = Style::new().fg(app.theme.muted);
    let (symbol, _, color) = status(app, build);
    Line::from(vec![
        Span::styled(format!("{symbol}  "), Style::new().fg(color)),
        Span::raw(build.definition.name.clone()),
        Span::styled(format!("  {}", branch(build)), muted),
    ])
}

pub fn status(app: &App, build: &Build) -> (&'static str, &'static str, Color) {
    let theme = &app.theme;
    match (build.status.as_str(), build.result.as_deref()) {
        ("inProgress", _) => ("⟳", "running", theme.build_running),
        ("cancelling", _) => ("⟳", "cancelling", theme.muted),
        ("completed", Some("succeeded")) => ("✔", "succeeded", theme.build_succeeded),
        ("completed", Some("partiallySucceeded")) => ("◑", "partially succeeded", theme.pr_waiting),
        ("completed", Some("failed")) => ("✖", "failed", theme.build_failed),
        ("completed", Some("canceled")) => ("⊘", "canceled", theme.muted),
        ("completed", _) => ("–", "completed", theme.muted),
        _ => ("○", "queued", theme.muted),
    }
}

fn branch(build: &Build) -> &str {
    build
        .source_branch
        .strip_prefix("refs/heads/")
        .unwrap_or(&build.source_branch)
}

/// `7m 35s` between start and finish.
fn duration(build: &Build) -> Option<String> {
    let parse = |date: &Option<String>| DateTime::parse_from_rfc3339(date.as_deref()?).ok();
    let seconds = (parse(&build.finish_time)? - parse(&build.start_time)?).num_seconds();
    Some(format!("{}m {}s", seconds / 60, seconds % 60))
}
