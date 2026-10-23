use chrono::{DateTime, Datelike, Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthChar;

use super::{field, heading, pad_right, short_date, text_width};
use crate::api::WorkItem;
use crate::app::App;

const MAX_BAR: usize = 30;
/// Narrowest lane worth drawing; below this the board is listed by column instead.
const MIN_LANE_WIDTH: usize = 16;

/// A board column with the sprint items in it.
struct Lane<'a> {
    name: String,
    cards: Vec<&'a WorkItem>,
}

/// Shown in the detail panel while the board is toggled on and nothing is open.
pub fn render_board(frame: &mut Frame, app: &App, area: Rect) {
    let muted = Style::new().fg(app.theme.muted);
    app.detail_height.set(area.height);
    app.detail_max_scroll.set(0);
    let Some(items) = &app.data.board_items else {
        frame.render_widget(Paragraph::new(Line::styled("Loading…", muted)), area);
        return;
    };
    let lanes = lanes(app, items);
    let name = app.data.sprint_name.as_deref().unwrap_or("Sprint board");
    let heading = heading(app, &format!("{name} · {} items", items.len()), area.width);
    if lanes.is_empty() {
        let lines = vec![heading, Line::styled("No items in the sprint", muted)];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    if !lanes_fit(area.width, lanes.len()) {
        let mut lines = vec![heading];
        lines.extend(board_list(app, &lanes, area.width));
        let max = lines.len().saturating_sub(usize::from(area.height));
        let max = u16::try_from(max).unwrap_or(u16::MAX);
        app.detail_max_scroll.set(max);
        let scroll = app.detail_scroll.min(max);
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
        return;
    }

    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(heading), head);
    let count = u32::try_from(lanes.len()).unwrap_or(1);
    let columns = Layout::horizontal(vec![Constraint::Ratio(1, count); lanes.len()]).split(body);
    for (lane, rect) in lanes.iter().zip(columns.iter()) {
        let block = Block::bordered()
            .title(format!(" {} ({}) ", lane.name, lane.cards.len()))
            .title_style(Style::new().fg(app.theme.title))
            .border_style(Style::new().fg(app.theme.border));
        let inner = block.inner(*rect);
        frame.render_widget(block, *rect);
        let rows = usize::from(inner.height);
        let width = usize::from(inner.width);
        let shown = if lane.cards.len() > rows {
            rows.saturating_sub(1)
        } else {
            lane.cards.len()
        };
        let mut lines: Vec<Line<'static>> = lane.cards[..shown]
            .iter()
            .map(|item| card(app, item, width))
            .collect();
        if shown < lane.cards.len() {
            let hidden = lane.cards.len() - shown;
            lines.push(Line::styled(format!("+{hidden} more"), muted));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Whether `count` lanes side by side are each at least [`MIN_LANE_WIDTH`] wide.
fn lanes_fit(width: u16, count: usize) -> bool {
    usize::from(width) >= count * MIN_LANE_WIDTH
}

/// The sprint items per board column, in board order; items in columns the board lacks follow.
fn lanes<'a>(app: &App, items: &'a [WorkItem]) -> Vec<Lane<'a>> {
    let mut lanes: Vec<Lane> = app
        .board_columns()
        .into_iter()
        .map(|name| Lane {
            name,
            cards: Vec::new(),
        })
        .collect();
    for item in items {
        let name = column_of(item);
        match lanes.iter_mut().find(|lane| lane.name == name) {
            Some(lane) => lane.cards.push(item),
            None => lanes.push(Lane {
                name,
                cards: vec![item],
            }),
        }
    }
    lanes
}

/// The board column of `item`, else its state.
fn column_of(item: &WorkItem) -> String {
    let fields = &item.fields;
    [fields.board_column.as_deref(), Some(&fields.state)]
        .into_iter()
        .flatten()
        .find(|name| !name.is_empty())
        .unwrap_or("Other")
        .to_string()
}

/// The lanes one below the other, for areas too narrow to put them side by side.
fn board_list(app: &App, lanes: &[Lane], width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for lane in lanes {
        lines.push(Line::default());
        lines.push(heading(
            app,
            &format!("{} ({})", lane.name, lane.cards.len()),
            width,
        ));
        for item in &lane.cards {
            let mut card = card(app, item, usize::from(width).saturating_sub(2));
            card.spans.insert(0, Span::raw("  "));
            lines.push(card);
        }
    }
    lines
}

/// `#id title` cut to `width` columns, with the assignee's initials after it.
fn card(app: &App, item: &WorkItem, width: usize) -> Line<'static> {
    let fields = &item.fields;
    let done = if fields.board_column_done == Some(true) {
        "✓ "
    } else {
        ""
    };
    let initials = fields
        .assigned_to
        .as_ref()
        .map(|assignee| initials(&assignee.display_name))
        .filter(|initials| !initials.is_empty())
        .map(|initials| format!(" {initials}"))
        .unwrap_or_default();
    let head_width = width.saturating_sub(text_width(&initials));
    let head = truncate(&format!("{done}#{} {}", item.id, fields.title), head_width);
    let mine = app.user.as_ref().is_some_and(|user| {
        fields
            .assigned_to
            .as_ref()
            .is_some_and(|assignee| assignee.id == user.id)
    });
    let style = if mine {
        Style::new().fg(app.theme.border_focused)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(pad_right(&head, head_width), style),
        Span::styled(initials, Style::new().fg(app.theme.muted)),
    ])
}

/// Up to two capital letters from the first words of `name`.
fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// `text` cut to `width` columns, ending in `…` when something was left out.
fn truncate(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// Shown in the detail panel when nothing is open.
pub fn overview(app: &App, width: u16) -> Vec<Line<'static>> {
    let muted = Style::new().fg(app.theme.muted);
    let data = &app.data;
    let Some(sprint) = &data.sprint_name else {
        return vec![Line::styled("Select an item and press enter", muted)];
    };
    let mut lines = vec![heading(app, sprint, width)];
    if let (Some(start), Some(finish)) = (&data.sprint_start, &data.sprint_finish) {
        let day = |date: &str| short_date(date).get(..10).unwrap_or_default().to_string();
        lines.push(field(
            app,
            "Dates",
            vec![Span::raw(format!("{} – {}", day(start), day(finish)))],
        ));
    }
    if let Some(days) = data.sprint_finish.as_deref().and_then(work_days_left) {
        lines.push(field(
            app,
            "Left",
            vec![Span::raw(format!("{days} work days"))],
        ));
    }

    let columns = my_columns(app);
    if !columns.is_empty() {
        lines.push(Line::default());
        lines.push(heading(app, "My items per column", width));
        let name_width = columns
            .iter()
            .map(|(name, _)| super::text_width(name))
            .max();
        let name_width = name_width.unwrap_or(0);
        for (name, count) in columns {
            lines.push(Line::from(vec![
                Span::raw(format!(
                    "  {}  {count:>3}  ",
                    super::pad_right(&name, name_width)
                )),
                Span::styled(
                    "█".repeat(count.min(MAX_BAR)),
                    Style::new().fg(app.theme.border_focused),
                ),
            ]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::styled("Select an item and press enter", muted));
    lines
}

/// Weekdays from today through the finish date.
pub fn work_days_left(finish: &str) -> Option<usize> {
    let finish = DateTime::parse_from_rfc3339(finish).ok()?.date_naive();
    Some(work_days_between(Local::now().date_naive(), finish))
}

fn work_days_between(from: NaiveDate, to: NaiveDate) -> usize {
    from.iter_days()
        .take_while(|day| *day <= to)
        .filter(|day| day.weekday().number_from_monday() <= 5)
        .count()
}

/// My items counted per board column, in board order when the board is loaded.
fn my_columns(app: &App) -> Vec<(String, usize)> {
    let Some(items) = &app.data.my_work_items else {
        return Vec::new();
    };
    let mut columns: Vec<(String, usize)> = app
        .board_columns()
        .into_iter()
        .map(|name| (name, 0))
        .collect();
    for item in items {
        let column = item
            .fields
            .board_column
            .clone()
            .unwrap_or_else(|| item.fields.state.clone());
        match columns.iter_mut().find(|(name, _)| *name == column) {
            Some((_, count)) => *count += 1,
            None => columns.push((column, 1)),
        }
    }
    columns
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;

    use super::*;
    use crate::app::Screen;
    use crate::test_support::{config, work_item};

    fn board_app(items: Vec<WorkItem>) -> App {
        let mut app = App::new(Some(config()), None);
        app.screen = Screen::Main;
        app.board_view = true;
        app.data.sprint_name = Some("Sprint 1".into());
        app.data.board_items = Some(items);
        app
    }

    fn item(id: u32, state: &str, column: Option<&str>) -> WorkItem {
        let mut item = work_item(id, &format!("Item {id}"), "User Story", state);
        item.fields.board_column = column.map(String::from);
        item
    }

    fn rendered(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_board(frame, app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn names<'a>(lanes: &'a [Lane]) -> Vec<(&'a str, usize)> {
        lanes
            .iter()
            .map(|lane| (lane.name.as_str(), lane.cards.len()))
            .collect()
    }

    #[test]
    fn lanes_follow_first_seen_order_without_a_board() {
        let items = [
            item(1, "Active", Some("Doing")),
            item(2, "New", Some("Todo")),
            item(3, "Active", Some("Doing")),
        ];

        let lanes = lanes(&App::new(None, None), &items);

        assert_eq!(names(&lanes), [("Doing", 2), ("Todo", 1)]);
    }

    #[test]
    fn item_without_a_board_column_goes_to_its_state_or_other() {
        let mut stateless = item(3, "", None);
        stateless.fields.state = String::new();
        let items = [item(1, "Resolved", None), stateless];

        let lanes = lanes(&App::new(None, None), &items);

        assert_eq!(names(&lanes), [("Resolved", 1), ("Other", 1)]);
    }

    #[test]
    fn lanes_need_the_minimum_width_each() {
        assert!(lanes_fit(64, 4));
        assert!(!lanes_fit(63, 4));
        assert!(lanes_fit(16, 1));
    }

    #[test]
    fn titles_are_cut_by_display_width() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer title", 8), "a longe…");
        assert_eq!(truncate("日本語のタイトル", 7), "日本語…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn initials_are_the_first_letters_of_two_words() {
        assert_eq!(initials("Mona Me"), "MM");
        assert_eq!(initials("anna maria andersson"), "AM");
        assert_eq!(initials("Cher"), "C");
        assert_eq!(initials(""), "");
    }

    #[test]
    fn card_shows_id_title_initials_and_done_mark() {
        let app = App::new(None, None);
        let mut done = item(7, "Active", Some("Active"));
        done.fields.board_column_done = Some(true);
        done.fields.assigned_to = serde_json::from_value(json!({
            "id": "u1", "displayName": "Olle Other"
        }))
        .ok();

        let line = card(&app, &done, 30).to_string();

        assert_eq!(line, "✓ #7 Item 7                 OO");
    }

    #[test]
    fn my_card_is_highlighted() {
        let mut app = App::new(Some(config()), None);
        app.user = serde_json::from_value(json!({
            "id": "me", "providerDisplayName": "Mona Me"
        }))
        .ok();
        let mut mine = item(1, "Active", None);
        let mut theirs = item(2, "Active", None);
        mine.fields.assigned_to =
            serde_json::from_value(json!({ "id": "me", "displayName": "Mona Me" })).ok();
        theirs.fields.assigned_to =
            serde_json::from_value(json!({ "id": "u2", "displayName": "Olle Other" })).ok();

        let mine = card(&app, &mine, 20);
        let theirs = card(&app, &theirs, 20);

        assert_eq!(mine.spans[0].style.fg, Some(app.theme.border_focused));
        assert_ne!(theirs.spans[0].style.fg, Some(app.theme.border_focused));
    }

    #[test]
    fn wide_areas_draw_one_titled_lane_per_column() {
        let app = board_app(vec![
            item(1, "New", Some("New")),
            item(2, "Active", Some("Active")),
            item(3, "Active", Some("Active")),
        ]);

        let screen = rendered(&app, 100, 10);

        assert!(screen.contains("Sprint 1 · 3 items"), "{screen}");
        let column = |line: &str, text: &str| line.find(text).map(|at| line[..at].chars().count());
        let titles = screen.lines().nth(1).unwrap();
        let new = column(titles, "New (1)").expect("New lane");
        let active = column(titles, "Active (2)").expect("Active lane");
        assert!(new < active, "{screen}");
        let card = screen.lines().find(|l| l.contains("#3 Item 3")).unwrap();
        // The title starts two columns after the lane's left border, the card one.
        assert!(column(card, "#3").unwrap() + 1 >= active, "{screen}");
        let card = screen.lines().find(|l| l.contains("#1 Item 1")).unwrap();
        assert!(column(card, "#1").unwrap() < active, "{screen}");
    }

    #[test]
    fn lane_with_too_many_cards_counts_the_hidden_ones() {
        let items = (1..=6).map(|id| item(id, "New", Some("New"))).collect();
        let app = board_app(items);

        let screen = rendered(&app, 40, 6);

        assert!(screen.contains("+4 more"), "{screen}");
    }

    #[test]
    fn narrow_areas_list_the_cards_under_a_heading_per_column() {
        let mut items = vec![
            item(1, "New", Some("New")),
            item(2, "Closed", Some("Closed")),
        ];
        items.push(item(3, "Active", Some("Active")));
        items.push(item(4, "Resolved", Some("Resolved")));
        items.push(item(5, "Done", Some("Done")));
        let app = board_app(items);

        let screen = rendered(&app, 50, 30);

        for heading in [
            "New (1)",
            "Closed (1)",
            "Active (1)",
            "Resolved (1)",
            "Done (1)",
        ] {
            assert!(screen.contains(heading), "{heading}\n{screen}");
        }
        assert!(screen.contains("  #1 Item 1"), "{screen}");
    }

    #[test]
    fn narrow_list_scrolls_with_the_detail_scroll() {
        let items = (1..=20)
            .map(|id| item(id, "New", Some(&format!("Column {id}"))))
            .collect();
        let mut app = board_app(items);

        let top = rendered(&app, 50, 8);
        let max = app.detail_max_scroll.get();
        app.detail_scroll = max;
        let bottom = rendered(&app, 50, 8);

        assert!(max > 0);
        assert!(top.contains("Column 1 (1)"), "{top}");
        assert!(!bottom.contains("Column 1 (1)"), "{bottom}");
        assert!(bottom.contains("Column 20 (1)"), "{bottom}");
    }

    #[test]
    fn board_waits_for_items_and_handles_an_empty_sprint() {
        let mut app = board_app(Vec::new());
        assert!(rendered(&app, 60, 6).contains("No items in the sprint"));

        app.data.board_items = None;

        assert!(rendered(&app, 60, 6).contains("Loading…"));
    }
}
