mod build_detail;
mod popup;
mod pr_detail;
mod pr_files;
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
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use ratatui_image::StatefulImage;

use crate::api::{PullRequest, ReviewSignals, WorkItem};
use crate::app::{
    App, Detail, DetailInfo, DetailTab, InboxEntry, LEFT_PANELS, Panel, Popup, Screen, Selection,
    Signals, SortKind, TextTarget,
};
use crate::images::{ImageState, file_name, image_marker};
use crate::rich_text::{Marks, RichText};

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
            let selection = Selection {
                items: picker.items(),
                selected: picker.selection.selected,
            };
            popup::render_picker(frame, app, &title, &selection, main);
        }
        Some(Popup::Confirm { prompt, .. }) => popup::render_confirm(frame, app, prompt, main),
        Some(Popup::Tags(picker)) => {
            let items = picker
                .selection
                .items
                .iter()
                .enumerate()
                .map(|(index, tag)| {
                    if picker.is_new(index) {
                        format!("+ create \"{tag}\"")
                    } else if picker.is_current(index) {
                        format!("✓ {tag}")
                    } else {
                        format!("  {tag}")
                    }
                })
                .collect();
            let selection = Selection {
                items,
                selected: picker.selection.selected,
            };
            let (before, after) = picker.query.split();
            let title = format!(" Tag #{}: {before}▏{after}", picker.work_item_id);
            popup::render_picker(frame, app, &title, &selection, main);
        }
        Some(Popup::Sprint(picker)) => {
            let items = picker
                .selection
                .items
                .iter()
                .map(|path| {
                    let Some(sprint) = picker.sprint(path) else {
                        return path.clone();
                    };
                    let day = |date: &Option<String>| {
                        date.as_deref()
                            .map(|date| short_date(date).get(..10).unwrap_or_default().to_string())
                    };
                    let dates = match (
                        day(&sprint.attributes.start_date),
                        day(&sprint.attributes.finish_date),
                    ) {
                        (Some(start), Some(finish)) => format!("  {start} – {finish}"),
                        _ => String::new(),
                    };
                    let current = if sprint.is_current() {
                        "  (current)"
                    } else {
                        ""
                    };
                    format!("{}{dates}{current}", sprint.name)
                })
                .collect();
            let selection = Selection {
                items,
                selected: picker.selection.selected,
            };
            let (before, after) = picker.query.split();
            let title = format!(" Sprint: {before}▏{after}");
            popup::render_picker(frame, app, &title, &selection, main);
        }
        Some(Popup::Text { target, text }) => {
            let title = match target {
                TextTarget::WorkItemComment(id) => format!(" Comment #{id}"),
                TextTarget::PrThread {
                    pr,
                    thread_id: Some(_),
                } => format!(" Reply on !{}", pr.pull_request_id),
                TextTarget::PrThread {
                    pr,
                    thread_id: None,
                } => {
                    format!(" New thread on !{}", pr.pull_request_id)
                }
                TextTarget::PrTitle(pr) => format!(" Title of !{}", pr.pull_request_id),
                TextTarget::PrDescription(pr) => {
                    format!(" Description of !{}", pr.pull_request_id)
                }
                TextTarget::AddReviewer(pr) => {
                    format!(" Add reviewer to !{} (name or email)", pr.pull_request_id)
                }
                TextTarget::LinkWorkItem(pr) => {
                    format!(" Work item id to link to !{}", pr.pull_request_id)
                }
            };
            let title = format!("{title} · shift/alt+enter: new line ");
            popup::render_text_input(frame, app, &title, text, main);
        }
        Some(Popup::BuildFilter { text }) => {
            popup::render_text_input(frame, app, " Filter pipelines ", text, main);
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
        Some(Popup::Vote { pr, selection }) => {
            let title = format!(" Vote on !{} ", pr.pull_request_id);
            popup::render_picker(frame, app, &title, selection, main);
        }
        Some(Popup::ThreadStatus { pr, selection, .. }) => {
            let title = format!(" Thread status on !{} ", pr.pull_request_id);
            popup::render_picker(frame, app, &title, selection, main);
        }
        Some(Popup::PrActions { pr, selection, .. }) => {
            let title = format!(" !{} ", pr.pull_request_id);
            popup::render_picker(frame, app, &title, selection, main);
        }
        Some(Popup::RemoveReviewer { pr, selection }) => {
            let title = format!(" Remove reviewer from !{} ", pr.pull_request_id);
            popup::render_picker(frame, app, &title, selection, main);
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
    if panel == Panel::Builds && !app.data.build_filter.is_empty() {
        title.push_str(&format!(" · \"{}\"", app.data.build_filter));
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
    app.detail_links.borrow_mut().clear();
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
    let thread_row = selected_thread_row(&lines, width);
    app.thread_row.set(thread_row);
    let total: usize = lines.iter().map(|line| wrapped_rows(line, width)).sum();
    let max = u16::try_from(total.saturating_sub(usize::from(content.height))).unwrap_or(u16::MAX);
    app.detail_max_scroll.set(max);
    app.detail_height.set(content.height);
    let scroll = match detail {
        Detail::PullRequest(_) if app.detail_tab == DetailTab::Files => {
            pr_files::list_scroll(app, content.height, content.width)
        }
        _ => None,
    }
    .unwrap_or(
        match thread_row {
            Some(row) if app.scroll_to_thread => row,
            _ => app.detail_scroll,
        }
        .min(max),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        content,
    );
    for slot in slots {
        let top = i32::from(slot.row) - i32::from(scroll);
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
    let mut row = 0;
    for line in lines {
        let text = line.to_string();
        let Some(url) = image_marker(&text).map(String::from) else {
            row += wrapped_rows(&line, width);
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
                let filler = Line::from(prefix);
                slots.push(ImageSlot {
                    url,
                    row: u16::try_from(row).unwrap_or(u16::MAX),
                    indent: filler.width() as u16,
                    cols,
                    rows,
                });
                row += wrapped_rows(&filler, width) * usize::from(rows);
                out.extend((0..rows).map(|_| filler.clone()));
            }
            Err(placeholder) => {
                let mut spans = prefix;
                spans.push(Span::styled(placeholder, muted));
                let line = Line::from(spans);
                row += wrapped_rows(&line, width);
                out.push(line);
            }
        }
    }
    (out, slots)
}

/// Row of the header of the selected comment thread, if one is drawn.
fn selected_thread_row(lines: &[Line<'static>], width: u16) -> Option<u16> {
    let index = lines.iter().position(|line| {
        line.spans
            .first()
            .is_some_and(|span| span.content == pr_detail::SELECTED_THREAD)
    })?;
    let row: usize = lines[..index]
        .iter()
        .map(|line| wrapped_rows(line, width))
        .sum();
    Some(u16::try_from(row).unwrap_or(u16::MAX))
}

/// Rows `line` takes when wrapped to `width`; lines wrap independently with `trim: false`.
fn wrapped_rows(line: &Line<'static>, width: u16) -> usize {
    Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width)
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
        (DetailTab::Files, _) => match app.files.as_ref().and_then(|f| f.changes.as_ref()) {
            Some(changes) => changes.files.len().to_string(),
            None => "…".into(),
        },
        (DetailTab::Comments, Some(DetailInfo::WorkItem(info))) => info.comment_count.to_string(),
        (DetailTab::Children, Some(DetailInfo::WorkItem(info))) => info.children.len().to_string(),
        (DetailTab::Links, Some(DetailInfo::WorkItem(info))) => {
            (info.links.len() + info.pull_requests.len()).to_string()
        }
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
    let rule = (width as usize).saturating_sub(text_width(text) + 2);
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

fn branch(ref_name: &str) -> &str {
    ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name)
}

/// Word-wraps `text` to `width`, starting every line with `prefix` and keeping each line's indent.
#[cfg(test)]
fn wrap(text: &str, width: u16, prefix: Span<'static>) -> Vec<Line<'static>> {
    let lines = text.lines().map(|line| Line::raw(line.to_string()));
    wrap_lines(lines, width, prefix)
}

/// Formatted text styled with the theme and wrapped to `width`.
fn rich_lines(app: &App, text: &RichText, width: u16, prefix: Span<'static>) -> Vec<Line<'static>> {
    let base = {
        let mut links = app.detail_links.borrow_mut();
        links.extend(text.links.iter().cloned());
        links.len() - text.links.len()
    };
    let lines = text.lines.iter().map(|segments| {
        Line::from(
            segments
                .iter()
                .map(|segment| {
                    let mut style = rich_style(app, segment.marks);
                    if segment
                        .link
                        .is_some_and(|link| Some(base + link) == app.link_index)
                    {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    Span::styled(segment.text.clone(), style)
                })
                .collect::<Vec<_>>(),
        )
    });
    wrap_lines(lines, width, prefix)
}

fn rich_style(app: &App, marks: Marks) -> Style {
    let theme = &app.theme;
    let mut style = Style::new();
    if marks.heading {
        style = style.fg(theme.title).add_modifier(Modifier::BOLD);
    }
    if marks.strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if marks.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if marks.strike {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if marks.code {
        style = style.fg(theme.toolbar_key);
    }
    if marks.link {
        style = style
            .fg(theme.build_running)
            .add_modifier(Modifier::UNDERLINED);
    }
    if marks.muted {
        style = style.fg(theme.muted);
    }
    style
}

/// Word-wraps styled lines, keeping each line's indent (plus list bullet) on continuation lines.
fn wrap_lines(
    lines: impl Iterator<Item = Line<'static>>,
    width: u16,
    prefix: Span<'static>,
) -> Vec<Line<'static>> {
    let available = (width as usize).saturating_sub(prefix.width()).max(10);
    let mut out = Vec::new();
    let mut push = |chars: Vec<(char, Style)>| out.push(styled_line(&prefix, chars));
    for line in lines {
        let source = line.to_string();
        if image_marker(&source).is_some() {
            push(source.trim().chars().map(|c| (c, Style::new())).collect());
            continue;
        }
        let chars: Vec<(char, Style)> = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
            .collect();
        let end = chars
            .iter()
            .rposition(|(c, _)| !c.is_whitespace())
            .map_or(0, |i| i + 1);
        if chars_width(&chars[..end]) <= available {
            push(chars[..end].to_vec());
            continue;
        }
        let indent = source.chars().take_while(|c| c.is_whitespace()).count();
        let bullet = if source.trim_start().starts_with("• ") {
            2
        } else {
            0
        };
        let padding = vec![(' ', Style::new()); (indent + bullet).min(available / 2)];
        let mut current = vec![(' ', Style::new()); indent.min(available / 2)];
        for word in chars.split(|(c, _)| c.is_whitespace()) {
            if word.is_empty() {
                continue;
            }
            let mut word = word.to_vec();
            loop {
                let used = chars_width(&current);
                let blank = current.iter().all(|(c, _)| c.is_whitespace());
                let space = usize::from(!blank);
                if used + space + chars_width(&word) <= available {
                    if !blank {
                        current.push((' ', Style::new()));
                    }
                    current.extend(word);
                    break;
                }
                if blank {
                    let rest = word.split_off(split_index(&word, available - used));
                    current.extend(word);
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
    out
}

/// Display width of `text` in terminal columns.
pub(crate) fn text_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Pads `text` with spaces to `width` columns (display width, not chars).
pub(super) fn pad_right(text: &str, width: usize) -> String {
    format!(
        "{text}{}",
        " ".repeat(width.saturating_sub(text_width(text)))
    )
}

fn chars_width(chars: &[(char, Style)]) -> usize {
    chars.iter().map(|(c, _)| c.width().unwrap_or(0)).sum()
}

/// Index at which `word` has to be split so the head fits `max` columns (at least one char).
fn split_index(word: &[(char, Style)], max: usize) -> usize {
    let mut used = 0;
    for (i, (c, _)) in word.iter().enumerate() {
        used += c.width().unwrap_or(0);
        if used > max {
            return i.max(1);
        }
    }
    word.len()
}

fn styled_line(prefix: &Span<'static>, chars: Vec<(char, Style)>) -> Line<'static> {
    let mut spans = vec![prefix.clone()];
    let mut current = String::new();
    let mut current_style = Style::new();
    for (c, style) in chars {
        if style != current_style && !current.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut current), current_style));
        }
        current_style = style;
        current.push(c);
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, current_style));
    }
    if spans.len() == 1 {
        spans.push(Span::raw(""));
    }
    Line::from(spans)
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
        Panel::Inbox => (data.my_prs().is_some() && data.other_prs().is_some()).then(|| {
            data.inbox()
                .iter()
                .map(|entry| inbox_line(app, entry))
                .collect()
        }),
        Panel::MyPrs => data.my_prs().map(|prs| pr_lines(app, prs.iter(), false)),
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
        Panel::Builds => data.shown_builds().map(|builds| {
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
    let width = |spans: &[Span]| spans.iter().map(Span::width).sum();
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
        .pr_signals()
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
    let mut push = |text: String, color: Color| spans.push(marker(text, color));
    if review.needs_vote {
        push("◉".into(), theme.pr_waiting);
    }
    if review.changed_since_vote {
        push("↻".into(), theme.build_running);
    }
    if review.replies > 0 {
        push(format!("↩{}", review.replies), theme.comment_author);
    }
    if review.mentions > 0 {
        push(format!("@{}", review.mentions), theme.title);
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
            spans.extend(pr_markers(app, pr));
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
    use crate::app::TextTarget;
    use crate::app::{Retry, SetupStep, TextInput};
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
        app.data.set_my_prs(vec![pull_request(
            1,
            "Add order filter",
            "me",
            "2026-10-01T10:00:00Z",
        )]);
        app.data.set_other_prs(Vec::new());
        app.data.my_work_items = Some(vec![work_item(10, "Order list", "User Story", "Active")]);
        app.data.sprint_name = Some("Sprint 41".into());
        app
    }

    fn pr_details() -> PullRequestDetails {
        let mut threads: Vec<crate::api::Thread> = vec![
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
        ];
        for comment in threads.iter_mut().flat_map(|thread| &mut thread.comments) {
            comment.text = RichText::from_markdown(comment.content.as_deref().unwrap_or_default());
        }
        PullRequestDetails {
            description: None,
            threads,
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
            description: Some(RichText::from_html("Show the orders")),
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
            links: Vec::new(),
        }
    }

    #[test]
    fn inbox_line_has_one_space_after_the_marker() {
        let app = main_app();
        let mut pr = pull_request(101, "Review me", "other", "2026-10-01T10:00:00Z");
        pr.review.needs_vote = true;
        let entry = InboxEntry {
            detail: Detail::PullRequest(Box::new(pr)),
            since: String::new(),
        };

        let line = inbox_line(&app, &entry).to_string();

        assert!(line.contains("!101  ◉ Review me"), "{line:?}");
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
    fn wrap_keeps_wide_characters_within_the_width() {
        let lines = wrap(
            "修复登录页面的错误 🚀 fix the login 🚀🚀🚀 page 修复修复修复修复修复修复修复修复修复修复",
            12,
            Span::raw("│ "),
        );

        assert!(lines.len() > 3);
        assert!(lines.iter().all(|line| line.width() <= 12), "{lines:?}");
    }

    #[test]
    fn wrap_splits_a_wide_word_by_width_never_mid_glyph() {
        let lines = wrap("修复登录页面的错误修复登录页面的错误", 11, Span::raw(""));

        assert_eq!(
            text(&lines),
            ["修复登录页", "面的错误修", "复登录页面", "的错误"]
        );
    }

    #[test]
    fn heading_rule_fills_the_width_with_wide_text() {
        let app = main_app();

        let line = heading(&app, "修复 🚀", 30);

        assert_eq!(line.width(), 30);
    }

    #[test]
    fn pr_titles_start_in_the_same_column_with_wide_markers() {
        let app = main_app();
        let prs = [
            pull_request(7, "修复 🚀", "me", "2026-10-01T10:00:00Z"),
            pull_request(1234, "Fix", "me", "2026-10-01T10:00:00Z"),
        ];

        let lines = pr_lines(&app, prs.iter(), false);

        let columns: Vec<_> = lines
            .iter()
            .zip(["修复 🚀", "Fix"])
            .map(|(line, title)| {
                let line = line.to_string();
                text_width(&line[..line.find(title).unwrap()])
            })
            .collect();
        assert_eq!(columns[0], columns[1]);
    }

    #[test]
    fn pad_right_pads_by_display_width() {
        assert_eq!(pad_right("修复", 6), "修复  ");
        assert_eq!(text_width(&pad_right("🚀", 5)), 5);
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
    fn detail_max_scroll_is_content_rows_beyond_the_viewport() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyPrs, 0);
        app.detail_scroll = 500;

        let first = screen(&app);

        assert!(app.detail_height.get() > 0);
        assert_eq!(app.detail_max_scroll.get(), 0);
        assert!(first.contains("feature/x → main"));

        let Some(Detail::PullRequest(pr)) = &mut app.detail else {
            panic!("expected a pull request");
        };
        pr.description = Some(
            (0..100)
                .map(|n| format!("Row {n}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        pr.render_description();
        let _ = screen(&app);

        assert!(app.detail_max_scroll.get() > 0);
        assert!(app.detail_max_scroll.get() < 200);
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
    fn branch_drops_heads_prefix_only() {
        assert_eq!(branch("refs/heads/feature/x"), "feature/x");
        assert_eq!(branch("refs/pull/1/merge"), "refs/pull/1/merge");
    }

    #[test]
    fn review_spans_mark_what_waits() {
        let app = main_app();
        let review = ReviewSignals {
            needs_vote: true,
            changed_since_vote: true,
            replies: 2,
            mentions: 1,
            ..Default::default()
        };

        let text: String = review_spans(&app, &review)
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(text, "◉ ↻ ↩2 @1 ");
        assert!(review_spans(&app, &ReviewSignals::default()).is_empty());
    }

    #[test]
    fn work_item_overview_shows_fields() {
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyWorkItems, 0);
        app.detail_info = Some(DetailInfo::WorkItem(Box::new(work_item_details())));

        let screen = screen(&app);

        assert!(screen.contains("Overview │ Children (3) │ Links (0) │ Comments (0)"));
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
        app.detail_info = Some(DetailInfo::WorkItem(Box::new(work_item_details())));
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
            input: TextInput::new("secret"),
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

    /// The previous quadratic placement, kept as an oracle for `place_images`.
    fn place_images_oracle(app: &App, lines: Vec<Line<'static>>, width: u16) -> Vec<u16> {
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut rows = Vec::new();
        for line in lines {
            let text = line.to_string();
            let Some(url) = image_marker(&text).map(String::from) else {
                out.push(line);
                continue;
            };
            let prefix = line.spans[..line.spans.len().saturating_sub(1)].to_vec();
            let ready = app.images.with(&url, |state| match state {
                Some(ImageState::Ready { rows, .. }) => Some(*rows),
                _ => None,
            });
            if let Some(height) = ready {
                let row = Paragraph::new(out.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(width);
                rows.push(u16::try_from(row).unwrap_or(u16::MAX));
                out.extend((0..height).map(|_| Line::from(prefix.clone())));
            } else {
                out.push(Line::from(format!("  [image: {}]", file_name(&url))));
            }
        }
        rows
    }

    fn image_app(ready: usize) -> App {
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Kitty);
        let app = App::new(Some(config()), Some(picker));
        for i in 0..ready {
            let url = format!("ready{i}");
            app.images.start_loading(vec![url.clone()]);
            app.images
                .insert(url, Ok(image::DynamicImage::new_rgb8(40, 40 + i as u32)));
        }
        app
    }

    fn mixed_lines(count: usize) -> Vec<Line<'static>> {
        let long =
            "a long line of prose that has to wrap more than once at narrow widths ".repeat(2);
        let mut lines = Vec::new();
        for i in 0..count {
            lines.push(Line::from(format!("{i} {long}")));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(crate::images::marker(&format!("ready{}", i % 3))),
            ]));
            if i % 4 == 0 {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(crate::images::marker("missing")),
                ]));
            }
            lines.push(Line::from("short"));
        }
        lines
    }

    #[test]
    fn place_images_matches_the_quadratic_oracle() {
        let app = image_app(3);
        for width in [1, 7, 20, 33, 80] {
            let lines = mixed_lines(12);

            let expected = place_images_oracle(&app, lines.clone(), width);
            let (placed, slots) = place_images(&app, lines, width);

            let rows: Vec<u16> = slots.iter().map(|slot| slot.row).collect();
            assert_eq!(rows, expected, "width {width}");
            assert!(!placed.is_empty());
        }
    }

    #[test]
    fn place_images_handles_many_images() {
        let app = image_app(3);
        let lines = mixed_lines(300);
        let width = 40;

        let expected = place_images_oracle(&app, lines.clone(), width);
        let (_, slots) = place_images(&app, lines, width);

        assert_eq!(slots.len(), 300);
        assert_eq!(
            slots.iter().map(|slot| slot.row).collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn place_images_first_row_is_zero_and_empty_input_is_empty() {
        let app = image_app(1);
        let marker = Line::from(crate::images::marker("ready0"));

        let (_, slots) = place_images(&app, vec![marker], 10);
        let (lines, none) = place_images(&app, Vec::new(), 10);

        assert_eq!(slots[0].row, 0);
        assert!(lines.is_empty() && none.is_empty());
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
        app.data.set_pr_signals(
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
        app.data.set_pr_signals(
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
        pr.description_text = Some(RichText::from_markdown(&format!(
            "Look:\n{}",
            crate::images::marker(
                "https://dev.azure.com/o/p/_apis/wit/attachments/1?fileName=shot.png"
            )
        )));
        app.detail = Some(Detail::PullRequest(Box::new(pr)));

        let screen = screen(&app);

        assert!(screen.contains("[image: shot.png]"));
        assert!(!screen.contains("[[image:"));
    }

    #[test]
    fn comment_popup_shows_pasted_markdown_lines() {
        let mut app = main_app();
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(10),
            text: TextInput::default(),
        });

        app.handle_paste("## Steps\r\n- open **list**\r\n- press q");

        let screen = screen(&app);
        assert!(screen.contains("Comment #10"));
        assert!(screen.contains("## Steps"));
        assert!(screen.contains("- open **list**"));
        assert!(screen.contains("- press q▏"));
    }

    fn files_app(diff: Option<(usize, Option<crate::diff::FileDiff>)>) -> App {
        use crate::api::{ChangeKind, ChangedFile, PullRequestChanges};
        let mut app = main_app();
        app.detail = app.data.detail(Panel::MyPrs, 0);
        app.detail_tab = DetailTab::Files;
        let file = |kind, path: &str, original_path: Option<&str>| ChangedFile {
            kind,
            path: path.into(),
            original_path: original_path.map(String::from),
        };
        app.files = Some(crate::app::Files {
            pr_id: 1,
            error: None,
            changes: Some(PullRequestChanges {
                base: "base".into(),
                head: "head".into(),
                files: vec![
                    file(ChangeKind::Modified, "/src/main.rs", None),
                    file(ChangeKind::Renamed, "/src/new.rs", Some("/src/old.rs")),
                    file(ChangeKind::Added, "/logo.png", None),
                ],
            }),
            selected: 1,
            diff,
        });
        app
    }

    #[test]
    fn files_tab_lists_changes_with_their_kind() {
        let screen = screen(&files_app(None));

        assert!(screen.contains("Files (3)"));
        assert!(screen.contains("M /src/main.rs"));
        assert!(screen.contains("R /src/old.rs → /src/new.rs"));
        assert!(screen.contains("A /logo.png"));
    }

    #[test]
    fn files_tab_shows_the_open_diff() {
        use crate::diff::{FileDiff, unified};
        let diff = unified(
            "fn main() {\n    old();\n}\n",
            "fn main() {\n    new();\n}\n",
        );

        let screen = screen(&files_app(Some((0, Some(FileDiff::Lines(diff))))));

        assert!(screen.contains("▍/src/main.rs"));
        assert!(screen.contains("@@ -1,3 +1,3 @@"));
        assert!(screen.contains("-    old();"));
        assert!(screen.contains("+    new();"));
        assert!(!screen.contains("/src/old.rs →"));
    }

    #[test]
    fn files_tab_says_when_a_file_cannot_be_diffed() {
        let skipped = screen(&files_app(Some((2, Some(crate::diff::FileDiff::Skipped)))));
        let loading = screen(&files_app(Some((2, None))));

        assert!(skipped.contains("Binary or too large to show"));
        assert!(loading.contains("▍/logo.png"));
        assert!(loading.contains("Loading…"));
    }

    #[test]
    fn file_list_scrolls_to_keep_the_selection_visible() {
        let mut app = files_app(None);
        app.files.as_mut().unwrap().selected = 2;

        assert_eq!(pr_files::list_scroll(&app, 2, 80), Some(1));
        assert_eq!(pr_files::list_scroll(&app, 20, 80), Some(0));
        // Wrapped paths take 2 + 3 + 1 rows.
        assert_eq!(pr_files::list_scroll(&app, 2, 12), Some(4));
        app.files.as_mut().unwrap().diff = Some((0, None));
        assert_eq!(pr_files::list_scroll(&app, 4, 80), None);
    }

    #[test]
    fn cursor_is_drawn_inside_the_text() {
        let mut app = main_app();
        app.popup = Some(Popup::BuildFilter {
            text: TextInput::new("abc"),
        });
        if let Some(Popup::BuildFilter { text }) = &mut app.popup {
            text.left();
        }

        assert!(screen(&app).contains("ab▏c"));
    }
}
