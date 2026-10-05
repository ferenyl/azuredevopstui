mod build_detail;
mod popup;
mod pr_detail;
mod setup;
mod sprint;
mod toolbar;
mod workitem_detail;

use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListState, Padding, Paragraph, Tabs, Wrap};

use ratatui_image::StatefulImage;

use crate::api::{PullRequest, ReviewSignals, WorkItem};
use crate::app::{
    App, Detail, DetailInfo, DetailTab, InboxEntry, LEFT_PANELS, Panel, Popup, Screen, Selection,
    Signals, SortKind,
};
use crate::images::{ImageState, file_name, image_marker};

const LABEL_WIDTH: usize = 13;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = &app.theme;
    frame.render_widget(
        Block::new().style(Style::new().bg(theme.background).fg(theme.foreground)),
        frame.area(),
    );
    let [main, toolbar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    toolbar::render(frame, app, toolbar_area);

    if let Screen::Setup(step) = &app.screen {
        setup::render(frame, app, step, main);
        return;
    }

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]).areas(main);
    let left_areas =
        Layout::vertical([Constraint::Ratio(1, LEFT_PANELS.len() as u32); LEFT_PANELS.len()])
            .split(left);

    for (panel, area) in LEFT_PANELS.iter().zip(left_areas.iter()) {
        render_panel(frame, app, *panel, *area);
    }
    render_panel(frame, app, Panel::Detail, right);
    match &app.popup {
        Some(Popup::Column(picker)) => {
            let title = if picker.unassign {
                format!(" Unassign #{} – new status ", picker.work_item_id)
            } else {
                format!(" Move #{} ", picker.work_item_id)
            };
            popup::render_picker(frame, app, &title, &picker.selection, main);
        }
        Some(Popup::Complete(picker)) => {
            let title = format!(" Complete !{} ", picker.pr.pull_request_id);
            popup::render_picker(frame, app, &title, &picker.selection, main);
        }
        Some(Popup::Sort { kind, selection }) => {
            let title = match kind {
                SortKind::PullRequests => " Sort PRs ",
                SortKind::WorkItems => " Sort work items ",
            };
            popup::render_picker(frame, app, title, selection, main);
        }
        Some(Popup::Types { selection, checked } | Popup::PrFilter { selection, checked }) => {
            let items = selection
                .items
                .iter()
                .zip(checked)
                .map(|(kind, checked)| format!("[{}] {kind}", if *checked { "x" } else { " " }))
                .collect();
            let selection = Selection {
                items,
                selected: selection.selected,
            };
            let title = if matches!(app.popup, Some(Popup::PrFilter { .. })) {
                " Filter others' PRs "
            } else {
                " Work item types "
            };
            popup::render_picker(frame, app, title, &selection, main);
        }
        None => {}
    }
    if app.show_help {
        popup::render_help(frame, app, main);
    }
}

fn render_panel(frame: &mut Frame, app: &App, panel: Panel, area: Rect) {
    let theme = &app.theme;
    let border = if app.focus == panel {
        theme.border_focused
    } else {
        theme.border
    };
    let items = panel_items(app, panel);
    let mut title = match (panel, &app.data.sprint_name, &app.detail) {
        (Panel::ReadyWorkItems, Some(sprint), _) => {
            match app
                .data
                .sprint_finish
                .as_deref()
                .and_then(sprint::work_days_left)
            {
                Some(days) => format!(" Ready – {sprint} · {days}d left"),
                None => format!(" Ready – {sprint}"),
            }
        }
        (Panel::Detail, _, Some(Detail::PullRequest(pr))) => {
            format!(" !{} {}", pr.pull_request_id, pr.title)
        }
        (Panel::Detail, _, Some(Detail::WorkItem(item))) => {
            format!(" #{} {}", item.id, item.fields.title)
        }
        (Panel::Detail, _, Some(Detail::Build(build))) => {
            format!(" {} {}", build.definition.name, build.build_number)
        }
        _ => format!(" {}", panel.title()),
    };
    if let Some(items) = &items {
        title.push_str(&format!(" ({})", items.len()));
    }
    if let (Some(kind), Some(config)) = (panel.sort_kind(), &app.config) {
        let sort = match kind {
            SortKind::PullRequests => config.sort.pull_requests.label(),
            SortKind::WorkItems => config.sort.work_items.label(),
        };
        title.push_str(&format!(" · {sort}"));
    }
    title.push(' ');
    let block = Block::bordered()
        .title(title)
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(border));

    if panel == Panel::Detail {
        render_detail(frame, app, block, area);
        return;
    }

    match items {
        Some(items) => {
            let selected = panel
                .index()
                .filter(|_| !items.is_empty())
                .map(|index| app.selections[index]);
            let highlight = if app.focus == panel {
                Style::new().bg(theme.selection_bg).fg(theme.selection_fg)
            } else {
                Style::new().bg(theme.selection_bg)
            };
            let list = List::new(items).block(block).highlight_style(highlight);
            let mut state = ListState::default().with_selected(selected);
            frame.render_stateful_widget(list, area, &mut state);
        }
        None => frame.render_widget(
            Paragraph::new("Loading…")
                .style(Style::new().fg(theme.muted))
                .block(block),
            area,
        ),
    }
}

fn render_detail(frame: &mut Frame, app: &App, block: Block, area: Rect) {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    let block = block.padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(detail) = &app.detail else {
        frame.render_widget(Paragraph::new(sprint::overview(app, inner.width)), inner);
        return;
    };

    let [tabs_area, rule_area, content] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    let tabs = detail.tabs();
    let titles = tabs.iter().map(|tab| tab_title(app, *tab));
    frame.render_widget(
        Tabs::new(titles)
            .select(tabs.iter().position(|tab| *tab == app.detail_tab))
            .style(muted)
            .highlight_style(
                Style::new()
                    .fg(theme.border_focused)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .divider(Span::styled("│", Style::new().fg(theme.border)))
            .padding(" ", " "),
        tabs_area,
    );
    frame.render_widget(
        Paragraph::new(Line::styled(
            "─".repeat(rule_area.width as usize),
            Style::new().fg(theme.border),
        )),
        rule_area,
    );

    let width = content.width;
    let lines = match (detail, &app.detail_info) {
        (Detail::PullRequest(pr), Some(DetailInfo::PullRequest(info))) => {
            pr_detail::lines(app, pr, Some(info), app.detail_tab, width)
        }
        (Detail::PullRequest(pr), _) => pr_detail::lines(app, pr, None, app.detail_tab, width),
        (Detail::WorkItem(item), Some(DetailInfo::WorkItem(info))) => {
            workitem_detail::lines(app, item, Some(info), app.detail_tab, width)
        }
        (Detail::WorkItem(item), _) => {
            workitem_detail::lines(app, item, None, app.detail_tab, width)
        }
        (Detail::Build(build), _) => build_detail::lines(app, build),
    };
    let (lines, slots) = place_images(app, lines, width);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        content,
    );
    for slot in slots {
        let top = i32::from(slot.row) - i32::from(app.detail_scroll);
        let fits = top >= 0 && top + i32::from(slot.rows) <= i32::from(content.height);
        if !fits || slot.indent >= content.width {
            continue;
        }
        let area = Rect::new(
            content.x + slot.indent,
            content.y + top as u16,
            slot.cols.min(content.width - slot.indent),
            slot.rows,
        );
        app.images.with(&slot.url, |state| {
            if let Some(ImageState::Ready { protocol, .. }) = state {
                frame.render_stateful_widget(StatefulImage::default(), area, &mut **protocol);
                app.images.drawn_at(area);
            }
        });
    }
}

/// Where a loaded image is drawn, in content rows.
struct ImageSlot {
    url: String,
    row: u16,
    indent: u16,
    cols: u16,
    rows: u16,
}

/// Swaps image marker lines for blank space (loaded images) or a placeholder line.
fn place_images(
    app: &App,
    lines: Vec<Line<'static>>,
    width: u16,
) -> (Vec<Line<'static>>, Vec<ImageSlot>) {
    let muted = Style::new().fg(app.theme.muted);
    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len());
    let mut slots = Vec::new();
    for line in lines {
        let text = line.to_string();
        let Some(url) = image_marker(&text).map(String::from) else {
            out.push(line);
            continue;
        };
        let prefix = line.spans[..line.spans.len().saturating_sub(1)].to_vec();
        let size = app.images.with(&url, |state| match state {
            Some(ImageState::Ready { cols, rows, .. }) => Ok((*cols, *rows)),
            Some(ImageState::Loading) => Err("⟳ loading image…".to_string()),
            _ => Err(format!("[image: {}]", file_name(&url))),
        });
        match size {
            Ok((cols, rows)) => {
                let row = Paragraph::new(out.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(width);
                slots.push(ImageSlot {
                    url,
                    row: u16::try_from(row).unwrap_or(u16::MAX),
                    indent: Line::from(prefix.clone()).width() as u16,
                    cols,
                    rows,
                });
                out.extend((0..rows).map(|_| Line::from(prefix.clone())));
            }
            Err(placeholder) => {
                let mut spans = prefix;
                spans.push(Span::styled(placeholder, muted));
                out.push(Line::from(spans));
            }
        }
    }
    (out, slots)
}

fn tab_title(app: &App, tab: DetailTab) -> String {
    let count = match (tab, &app.detail_info) {
        (DetailTab::Overview, _) => return tab.title().into(),
        (DetailTab::Comments, Some(DetailInfo::PullRequest(info))) => {
            info.threads.len().to_string()
        }
        (DetailTab::Checks, Some(DetailInfo::PullRequest(info))) => {
            (info.policies.len() + info.statuses.len()).to_string()
        }
        (DetailTab::Comments, Some(DetailInfo::WorkItem(info))) => info.comment_count.to_string(),
        (DetailTab::Children, Some(DetailInfo::WorkItem(info))) => info.children.len().to_string(),
        _ => "…".into(),
    };
    format!("{} ({count})", tab.title())
}

fn loading(app: &App) -> Vec<Line<'static>> {
    vec![Line::styled("Loading…", Style::new().fg(app.theme.muted))]
}

fn empty(app: &App, text: &str) -> Vec<Line<'static>> {
    vec![Line::styled(
        text.to_string(),
        Style::new().fg(app.theme.muted),
    )]
}

fn heading(app: &App, text: &str, width: u16) -> Line<'static> {
    let theme = &app.theme;
    let rule = (width as usize).saturating_sub(text.chars().count() + 2);
    Line::from(vec![
        Span::styled("▍", Style::new().fg(theme.title)),
        Span::styled(
            format!("{text} "),
            Style::new().fg(theme.title).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(rule), Style::new().fg(theme.border)),
    ])
}

fn field(app: &App, label: &str, value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<LABEL_WIDTH$}"),
        Style::new().fg(app.theme.muted),
    )];
    spans.extend(value);
    Line::from(spans)
}

fn badge(text: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!("● {text}"),
        Style::new().fg(color).add_modifier(Modifier::BOLD),
    )
}

/// Word-wraps `text` to `width`, starting every line with `prefix` and keeping each line's indent.
fn wrap(text: &str, width: u16, prefix: Span<'static>) -> Vec<Line<'static>> {
    let available = (width as usize).saturating_sub(prefix.width()).max(10);
    let mut lines = Vec::new();
    let mut push = |text: String| lines.push(Line::from(vec![prefix.clone(), Span::raw(text)]));
    for source in text.lines() {
        if image_marker(source).is_some() {
            push(source.trim().to_string());
            continue;
        }
        let indent = source.chars().take_while(|c| c.is_whitespace()).count();
        let padding = " ".repeat(indent.min(available / 2));
        let mut current = padding.clone();
        for word in source.split_whitespace() {
            let mut word: Vec<char> = word.chars().collect();
            loop {
                let used = current.chars().count();
                let blank = current.trim().is_empty();
                let space = usize::from(!blank);
                if used + space + word.len() <= available {
                    if !blank {
                        current.push(' ');
                    }
                    current.extend(&word);
                    break;
                }
                if blank {
                    let rest = word.split_off(available - used);
                    current.extend(&word);
                    push(std::mem::replace(&mut current, padding.clone()));
                    word = rest;
                    if word.is_empty() {
                        break;
                    }
                } else {
                    push(std::mem::replace(&mut current, padding.clone()));
                }
            }
        }
        push(current);
    }
    lines
}

fn type_color(app: &App, work_item_type: &str) -> Color {
    let theme = &app.theme;
    match work_item_type {
        "Bug" => theme.workitem_bug,
        "Task" => theme.workitem_task,
        _ => theme.workitem_story,
    }
}

fn state_color(app: &App, state: &str) -> Color {
    let theme = &app.theme;
    match state {
        "New" | "To Do" | "Proposed" => theme.muted,
        "Active" | "Committed" | "In Progress" | "Doing" => theme.build_running,
        "Resolved" | "Done" | "Closed" | "Completed" => theme.pr_approved,
        "Removed" => theme.error,
        _ => theme.foreground,
    }
}

/// `2026-10-01T14:49:33Z` -> `2026-10-01 16:49` in local time.
fn short_date(date: &str) -> String {
    match DateTime::parse_from_rfc3339(date) {
        Ok(date) => date
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => date.get(..16).unwrap_or(date).replace('T', " "),
    }
}

/// `None` while the panel's data is loading.
fn panel_items(app: &App, panel: Panel) -> Option<Vec<Line<'static>>> {
    let data = &app.data;
    match panel {
        Panel::Inbox => (data.my_prs.is_some() && data.other_prs.is_some()).then(|| {
            data.inbox()
                .iter()
                .map(|entry| inbox_line(app, entry))
                .collect()
        }),
        Panel::MyPrs => data
            .my_prs
            .as_ref()
            .map(|prs| pr_lines(app, prs.iter(), false)),
        Panel::OtherPrs => data
            .shown_other_prs()
            .map(|prs| pr_lines(app, prs.into_iter(), true)),
        Panel::MyWorkItems => data
            .my_work_items
            .as_ref()
            .map(|items| items.iter().map(|item| work_item_line(app, item)).collect()),
        Panel::ReadyWorkItems => data
            .ready_work_items
            .as_ref()
            .map(|items| items.iter().map(|item| work_item_line(app, item)).collect()),
        Panel::Builds => data.builds.as_ref().map(|builds| {
            builds
                .iter()
                .map(|build| build_detail::line(app, build))
                .collect()
        }),
        Panel::Detail => None,
    }
}

/// PR lines with the id and signal columns padded to the same width.
fn pr_lines<'a>(
    app: &App,
    prs: impl Iterator<Item = &'a PullRequest>,
    show_author: bool,
) -> Vec<Line<'static>> {
    let rows: Vec<_> = prs.map(|pr| (pr, pr_markers(app, pr))).collect();
    let width = |spans: &[Span]| spans.iter().map(|span| span.content.chars().count()).sum();
    let id_width = rows
        .iter()
        .map(|(pr, _)| pr.pull_request_id.to_string().len())
        .max()
        .unwrap_or(0);
    let signal_width = rows
        .iter()
        .map(|(_, signals)| width(signals))
        .max()
        .unwrap_or(0);
    rows.into_iter()
        .map(|(pr, signals)| {
            let padding = signal_width - width(&signals);
            pr_line(app, pr, signals, id_width, padding, show_author)
        })
        .collect()
}

fn pr_line(
    app: &App,
    pr: &PullRequest,
    signals: Vec<Span<'static>>,
    id_width: usize,
    padding: usize,
    show_author: bool,
) -> Line<'static> {
    let muted = Style::new().fg(app.theme.muted);
    let mut spans = vec![Span::styled(
        format!("!{:<id_width$}  ", pr.pull_request_id),
        muted,
    )];
    let has_column = padding > 0 || !signals.is_empty();
    spans.extend(signals);
    if has_column {
        spans.push(Span::raw(" ".repeat(padding + 1)));
    }
    spans.push(Span::raw(pr.title.clone()));
    if pr.is_draft {
        spans.push(Span::styled(" [draft]", muted));
    }
    let mut details = pr.repository.name.clone();
    if show_author {
        details.push_str(&format!(" · {}", pr.created_by.display_name));
    }
    spans.push(Span::styled(format!("  {details}"), muted));
    Line::from(spans)
}

/// Markers for my PR's signals or for what waits on me in someone else's PR.
fn pr_markers(app: &App, pr: &PullRequest) -> Vec<Span<'static>> {
    let mut spans = app
        .data
        .pr_signals
        .get(&pr.pull_request_id)
        .map(|signals| signal_spans(app, signals))
        .unwrap_or_default();
    spans.extend(review_spans(app, &pr.review));
    spans
}

fn marker(text: String, color: Color) -> Span<'static> {
    Span::styled(
        format!("{text} "),
        Style::new().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn review_spans(app: &App, review: &ReviewSignals) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = Vec::new();
    if review.needs_vote {
        spans.push(marker("◉".into(), theme.pr_waiting));
    }
    if review.changed_since_vote {
        spans.push(marker("↻".into(), theme.build_running));
    }
    if review.replies > 0 {
        spans.push(marker(format!("↩{}", review.replies), theme.comment_author));
    }
    if review.mentions > 0 {
        spans.push(marker(format!("@{}", review.mentions), theme.title));
    }
    spans
}

fn inbox_line(app: &App, entry: &InboxEntry) -> Line<'static> {
    let theme = &app.theme;
    let muted = Style::new().fg(theme.muted);
    let mut spans = vec![if app.data.is_new(&entry.detail) {
        marker("• ".into(), theme.title)
    } else {
        Span::raw("   ")
    }];
    match &entry.detail {
        Detail::PullRequest(pr) => {
            spans.push(Span::styled(format!("!{}  ", pr.pull_request_id), muted));
            spans.extend(
                pr_markers(app, pr)
                    .into_iter()
                    .map(|span| Span::styled(format!("{} ", span.content), span.style)),
            );
            spans.push(Span::raw(pr.title.clone()));
        }
        Detail::WorkItem(item) => {
            spans.push(Span::styled(
                format!("#{} ", item.id),
                Style::new().fg(type_color(app, &item.fields.work_item_type)),
            ));
            spans.push(marker("@ ".into(), theme.title));
            spans.push(Span::raw(item.fields.title.clone()));
        }
        Detail::Build(build) => {
            spans.extend(build_detail::line(app, build).spans);
        }
    }
    spans.push(Span::styled(format!("  {}", age(&entry.since)), muted));
    Line::from(spans)
}

/// Time since `date` as `5m`, `3h` or `2d`.
fn age(date: &str) -> String {
    let Ok(date) = DateTime::parse_from_rfc3339(date) else {
        return String::new();
    };
    let minutes = (Utc::now() - date.with_timezone(&Utc)).num_minutes().max(0);
    match minutes {
        0..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

/// Compact markers for what needs attention, each followed by a space.
fn signal_spans(app: &App, signals: &Signals) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = Vec::new();
    let mut push = |text: String, color: Color| spans.push(marker(text, color));
    if signals.unresolved > 0 {
        push(format!("✎{}", signals.unresolved), theme.pr_waiting);
    }
    if signals.waiting {
        push("◔".into(), theme.pr_waiting);
    }
    if signals.rejected {
        push("⊘".into(), theme.pr_rejected);
    }
    if signals.failed_checks {
        push("✖".into(), theme.build_failed);
    }
    if signals.conflicts {
        push("⇄".into(), theme.error);
    }
    if signals.unlinked {
        push("∅".into(), theme.pr_waiting);
    }
    if signals.ready {
        push("✔".into(), theme.pr_approved);
    }
    spans
}

fn work_item_line(app: &App, item: &WorkItem) -> Line<'static> {
    let fields = &item.fields;
    let priority = fields
        .priority
        .map(|p| format!("P{p} · "))
        .unwrap_or_default();
    Line::from(vec![
        Span::styled(
            format!("#{} ", item.id),
            Style::new().fg(type_color(app, &fields.work_item_type)),
        ),
        Span::raw(fields.title.clone()),
        Span::styled(
            format!("  {priority}{}", fields.state),
            Style::new().fg(app.theme.muted),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;

    use super::*;
    use crate::api::{PullRequestDetails, WorkItemDetails};
    use crate::app::{Retry, SetupStep};
    use crate::test_support::{config, pull_request, work_item};

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn main_app() -> App {
        let mut app = App::new(Some(config()), None);
        app.screen = Screen::Main;
        app.data = crate::app::Data {
            my_prs: Some(vec![pull_request(
                1,
                "Add order filter",
                "me",
                "2026-10-01T10:00:00Z",
            )]),
            other_prs: Some(Vec::new()),
            my_work_items: Some(vec![work_item(10, "Order list", "User Story", "Active")]),
            ready_work_items: None,
            sprint_name: Some("Sprint 41".into()),
            ..Default::default()
        };
        app
    }

    fn pr_details() -> PullRequestDetails {
        PullRequestDetails {
            threads: vec![
                serde_json::from_value(json!({
                    "status": "active",
                    "threadContext": { "filePath": "/src/main.rs" },
                    "comments": [{
                        "author": { "id": "u", "displayName": "Anna" },
                        "content": "Please rename this",
                        "publishedDate": "2026-10-01T10:00:00Z"
                    }]
                }))
                .unwrap(),
            ],
            statuses: vec![
                serde_json::from_value(json!({
                    "id": 1,
                    "state": "failed",
                    "context": { "name": "coverage", "genre": "ci" }
                }))
                .unwrap(),
            ],
            policies: vec![
                serde_json::from_value(json!({
                    "status": "approved",
                    "configuration": {
                        "isBlocking": true,
                        "type": { "displayName": "Build" },
                        "settings": { "displayName": "CI build" }
                    }
                }))
                .unwrap(),
            ],
            work_items: Vec::new(),
        }
    }

    fn work_item_details() -> WorkItemDetails {
        WorkItemDetails {
            state: "Active".into(),
            board_column: Some("Active".into()),
            board_column_done: true,
            assigned_to: Some("Anna".into()),
            iteration_path: Some("MyProject\\Sprint 41".into()),
            tags: Some("backend; urgent".into()),
            description: Some("Show the orders".into()),
            acceptance_criteria: None,
            repro_steps: None,
            comment_count: 0,
            comments: Vec::new(),
            children: vec![
                work_item(20, "Write API", "Task", "Active"),
                work_item(21, "Deploy", "Release Task", "New"),
                work_item(22, "Write UI", "Task", "Closed"),
            ],
            pull_requests: Vec::new(),
        }
    }

    #[test]
    fn wrap_breaks_on_words() {
        let lines = wrap("one two three four five", 10, Span::raw(""));

        assert_eq!(text(&lines), ["one two", "three four", "five"]);
    }

    #[test]
    fn wrap_never_goes_below_minimum_width() {
        let lines = wrap("three four", 3, Span::raw(""));

        assert_eq!(text(&lines), ["three four"]);
    }

    #[test]
    fn wrap_keeps_indent_on_continuation_lines() {
        let lines = wrap("  - alpha beta gamma", 14, Span::raw(""));

        assert_eq!(text(&lines), ["  - alpha beta", "  gamma"]);
    }

    #[test]
    fn wrap_splits_words_longer_than_the_width() {
        let lines = wrap("abcdefghijklmnop", 10, Span::raw(""));

        assert_eq!(text(&lines), ["abcdefghij", "klmnop"]);
    }

    #[test]
    fn wrap_keeps_blank_lines_and_prefix() {
        let lines = wrap("first\n\nsecond", 20, Span::raw("│ "));

        assert_eq!(text(&lines), ["│ first", "│ ", "│ second"]);
    }

    #[test]
    fn short_date_uses_local_time() {
        let date = "2026-10-01T14:49:33Z";
        let expected = DateTime::parse_from_rfc3339(date)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();

        assert_eq!(short_date(date), expected);
    }

    #[test]
    fn short_date_falls_back_for_unparsable_dates() {
        assert_eq!(short_date("2026-10-01T14:49:33"), "2026-10-01 14:49");
        assert_eq!(short_date("soon"), "soon");
    }

    #[test]
    fn tab_titles_show_counts_once_loaded() {
        let mut app = main_app();

        assert_eq!(tab_title(&app, DetailTab::Overview), "Overview");
        assert_eq!(tab_title(&app, DetailTab::Comments), "Comments (…)");
        app.detail_info = Some(DetailInfo::PullRequest(pr_details()));
        assert_eq!(tab_title(&app, DetailTab::Comments), "Comments (1)");
        assert_eq!(tab_title(&app, DetailTab::Checks), "Checks (2)");
    }

    #[test]
    fn main_view_shows_lists_with_counts_and_sort() {
        let screen = screen(&main_app());

        assert!(screen.contains("My PRs (1) · newest"));
        assert!(screen.contains("My work items (1) · priority"));
        assert!(screen.contains("Others' PRs (0)"));
        assert!(screen.contains("Ready – Sprint 41"));
        assert!(screen.contains("!1  Add order filter"));
        assert!(screen.contains("#10 Order list"));
        assert!(screen.contains("Loading…"));
        assert!(screen.contains("Select an item and press enter"));
    }

    #[test]
    fn pull_request_overview_shows_fields_and_reviewers() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyPrs, 0);

        let screen = screen(&app);

        assert!(screen.contains("Overview │ Comments (…) │ Checks (…)"));
        assert!(screen.contains("● active"));
        assert!(screen.contains("feature/x → main"));
        assert!(screen.contains("Anna  approved  required"));
    }

    #[test]
    fn checks_tab_shows_summary_and_sections() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyPrs, 0);
        app.detail_info = Some(DetailInfo::PullRequest(pr_details()));
        app.detail_tab = DetailTab::Checks;

        let screen = screen(&app);

        assert!(screen.contains("✔ 1 passed   ✖ 1 failed"));
        assert!(screen.contains("▍Policies"));
        assert!(screen.contains("✔ CI build"));
        assert!(screen.contains("✖ ci/coverage"));
    }

    #[test]
    fn comments_tab_shows_threads() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyPrs, 0);
        app.detail_info = Some(DetailInfo::PullRequest(pr_details()));
        app.detail_tab = DetailTab::Comments;

        let screen = screen(&app);

        assert!(screen.contains("● active  /src/main.rs"));
        assert!(screen.contains("│ Please rename this"));
    }

    #[test]
    fn work_item_overview_shows_fields() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyWorkItems, 0);
        app.detail_info = Some(DetailInfo::WorkItem(work_item_details()));

        let screen = screen(&app);

        assert!(screen.contains("Overview │ Children (3) │ Comments (0)"));
        assert!(screen.contains("Active · Done"));
        assert!(screen.contains("Assigned to  Anna"));
        assert!(screen.contains(" backend "));
        assert!(screen.contains(" urgent "));
        assert!(screen.contains("Show the orders"));
    }

    #[test]
    fn children_tab_groups_by_type() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyWorkItems, 0);
        app.detail_info = Some(DetailInfo::WorkItem(work_item_details()));
        app.detail_tab = DetailTab::Children;

        let screen = screen(&app);

        let release = screen.find("▍Release Task (1)").unwrap();
        let task = screen.find("▍Task (2)").unwrap();
        assert!(release < task);
        assert!(screen.contains("#20 Write API  ● Active"));
        assert!(screen.contains("#22 Write UI  ● Closed"));
    }

    #[test]
    fn help_popup_lists_keys() {
        let mut app = main_app();
        app.show_help = true;

        let screen = screen(&app);

        assert!(screen.contains(" Keys "));
        assert!(screen.contains("change box/column"));
        assert!(screen.contains("filter"));
    }

    #[test]
    fn pat_input_is_masked() {
        let mut app = App::new(None, None);
        app.screen = Screen::Setup(SetupStep::EnterPat {
            input: "secret".into(),
            error: Some("The saved token was rejected.".into()),
            cancelable: false,
        });

        let screen = screen(&app);

        assert!(screen.contains("******"));
        assert!(!screen.contains("secret"));
        assert!(screen.contains("The saved token was rejected."));
    }

    #[test]
    fn failed_setup_shows_error_and_retry() {
        let mut app = App::new(None, None);
        app.screen = Screen::Setup(SetupStep::Failed {
            error: "az login required".into(),
            retry: Retry::Authenticate,
        });

        let screen = screen(&app);

        assert!(screen.contains("Setup failed"));
        assert!(screen.contains("az login required"));
        assert!(screen.contains("[r] reload"));
    }

    #[test]
    fn wrap_keeps_image_markers_whole() {
        let text = "before\n  [[image:https://dev.azure.com/very/long/url/image.png]]\nafter";

        let lines = wrap(text, 12, Span::raw("│ "));

        assert_eq!(
            text_of(&lines),
            [
                "│ before",
                "│ [[image:https://dev.azure.com/very/long/url/image.png]]",
                "│ after"
            ]
        );
    }

    fn text_of(lines: &[Line]) -> Vec<String> {
        text(lines)
    }

    #[test]
    fn my_pull_requests_show_signals() {
        let mut app = main_app();
        app.data.pr_signals.insert(
            1,
            Signals {
                unresolved: 2,
                waiting: true,
                rejected: true,
                failed_checks: true,
                conflicts: true,
                unlinked: false,
                ready: false,
            },
        );

        let screen = screen(&app);

        assert!(screen.contains("!1  ✎2 ◔ ⊘ ✖ ⇄  Add order filter"));
    }

    #[test]
    fn ready_pull_request_shows_check() {
        let mut app = main_app();
        app.data.pr_signals.insert(
            1,
            Signals {
                ready: true,
                ..Signals::default()
            },
        );

        assert!(screen(&app).contains("!1  ✔  Add order filter"));
    }

    #[test]
    fn image_without_graphics_shows_file_name() {
        let mut app = main_app();
        let mut pr = pull_request(1, "PR", "me", "2026-10-01T10:00:00Z");
        pr.description = Some(format!(
            "Look:\n{}",
            crate::images::marker(
                "https://dev.azure.com/o/p/_apis/wit/attachments/1?fileName=shot.png"
            )
        ));
        app.detail = Some(Detail::PullRequest(Box::new(pr)));

        let screen = screen(&app);

        assert!(screen.contains("[image: shot.png]"));
        assert!(!screen.contains("[[image:"));
    }
}
