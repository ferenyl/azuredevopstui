mod pr_detail;
mod setup;
mod toolbar;
mod workitem_detail;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListState, Paragraph, Wrap};

use crate::api::{PullRequest, WorkItem};
use crate::app::{App, Detail, DetailInfo, LEFT_PANELS, Panel, Screen};

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
    let muted = Style::new().fg(app.theme.muted);
    let lines = match (&app.detail, &app.detail_info) {
        (Some(Detail::PullRequest(pr)), Some(DetailInfo::PullRequest(info))) => {
            pr_detail::lines(app, pr, Some(info))
        }
        (Some(Detail::PullRequest(pr)), _) => pr_detail::lines(app, pr, None),
        (Some(Detail::WorkItem(item)), Some(DetailInfo::WorkItem(info))) => {
            workitem_detail::lines(app, item, Some(info))
        }
        (Some(Detail::WorkItem(item)), _) => workitem_detail::lines(app, item, None),
        (None, _) => vec![Line::styled("Select an item and press enter", muted)],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0))
            .block(block),
        area,
    );
}

fn heading(app: &App, text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        Style::new()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    )
}

fn indented(text: &str, indent: usize) -> Vec<Line<'static>> {
    let padding = " ".repeat(indent);
    text.lines()
        .map(|line| Line::raw(format!("{padding}{line}")))
        .collect()
}

/// `2026-10-01T14:49:33Z` -> `2026-10-01 14:49` (UTC).
fn short_date(date: &str) -> String {
    date.get(..16).unwrap_or(date).replace('T', " ")
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
    let theme = &app.theme;
    let type_color = match item.fields.work_item_type.as_str() {
        "Bug" => theme.workitem_bug,
        "Task" => theme.workitem_task,
        _ => theme.workitem_story,
    };
    Line::from(vec![
        Span::styled(format!("#{} ", item.id), Style::new().fg(type_color)),
        Span::raw(item.fields.title.clone()),
        Span::styled(
            format!("  {}", item.fields.state),
            Style::new().fg(theme.muted),
        ),
    ])
}
