mod popup;
mod pr_detail;
mod setup;
mod toolbar;
mod workitem_detail;

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListState, Padding, Paragraph, Tabs, Wrap};

use crate::api::{PullRequest, WorkItem};
use crate::app::{App, Detail, DetailInfo, DetailTab, LEFT_PANELS, Panel, Popup, Screen, SortKind};

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
    let left_areas = Layout::vertical([Constraint::Ratio(1, 4); 4]).split(left);

    for (panel, area) in LEFT_PANELS.iter().zip(left_areas.iter()) {
        render_panel(frame, app, *panel, *area);
    }
    render_panel(frame, app, Panel::Detail, right);
    match &app.popup {
        Some(Popup::Column(picker)) => popup::render_picker(
            frame,
            app,
            &format!(" Move #{} ", picker.work_item_id),
            &picker.selection,
            main,
        ),
        Some(Popup::Sort { kind, selection }) => {
            let title = match kind {
                SortKind::PullRequests => " Sort PRs ",
                SortKind::WorkItems => " Sort work items ",
            };
            popup::render_picker(frame, app, title, selection, main);
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
        (Panel::ReadyWorkItems, Some(sprint), _) => format!(" Ready – {sprint}"),
        (Panel::Detail, _, Some(Detail::PullRequest(pr))) => {
            format!(" !{} {}", pr.pull_request_id, pr.title)
        }
        (Panel::Detail, _, Some(Detail::WorkItem(item))) => {
            format!(" #{} {}", item.id, item.fields.title)
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
        frame.render_widget(
            Paragraph::new(Line::styled("Select an item and press enter", muted)),
            inner,
        );
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
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        content,
    );
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
        Panel::MyPrs => data
            .my_prs
            .as_ref()
            .map(|prs| prs.iter().map(|pr| pr_line(app, pr, false)).collect()),
        Panel::OtherPrs => data
            .other_prs
            .as_ref()
            .map(|prs| prs.iter().map(|pr| pr_line(app, pr, true)).collect()),
        Panel::MyWorkItems => data
            .my_work_items
            .as_ref()
            .map(|items| items.iter().map(|item| work_item_line(app, item)).collect()),
        Panel::ReadyWorkItems => data
            .ready_work_items
            .as_ref()
            .map(|items| items.iter().map(|item| work_item_line(app, item)).collect()),
        Panel::Detail => None,
    }
}

fn pr_line(app: &App, pr: &PullRequest, show_author: bool) -> Line<'static> {
    let muted = Style::new().fg(app.theme.muted);
    let mut spans = vec![
        Span::styled(format!("!{} ", pr.pull_request_id), muted),
        Span::raw(pr.title.clone()),
    ];
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
