use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, HELP, PickerHit, Selection, TextInput};

const KEY_WIDTH: usize = 15;

pub fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let mut lines: Vec<Line> = Vec::new();
    for (heading, actions) in HELP {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(
            format!(" {heading}"),
            Style::new().fg(theme.title),
        ));
        lines.extend(actions.iter().map(|action| {
            Line::from(vec![
                Span::styled(
                    format!(" {:<KEY_WIDTH$}", action.key_label()),
                    Style::new().fg(theme.toolbar_key),
                ),
                Span::raw(action.label()),
            ])
        }));
    }
    let area = centered(area, 44, lines.len() as u16 + 2);
    let block = Block::bordered()
        .title(" Keys ")
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn render_errors(frame: &mut Frame, app: &App, first: usize, area: Rect) {
    const WIDTH: u16 = 80;
    let theme = &app.theme;
    let width = WIDTH.min(area.width);
    let errors: Vec<String> = app
        .activity
        .errors()
        .skip(first)
        .map(|(at, text)| format!("{} {text}", at.format("%H:%M:%S")))
        .collect();
    let rows: u16 = errors.iter().map(|error| wrapped_rows(error, width)).sum();
    let height = rows
        .max(1)
        .min(area.height.saturating_sub(4).max(1))
        .saturating_add(2);
    let area = centered(area, width, height);

    let lines: Vec<Line> = if errors.is_empty() {
        vec![Line::styled("no errors", Style::new().fg(theme.muted))]
    } else {
        errors.into_iter().map(Line::from).collect()
    };
    let title = format!(
        " Errors ({}) · j/k: scroll · esc: close ",
        app.activity.error_count()
    );
    let block = Block::bordered()
        .title(title)
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.error));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Rows `text` takes inside a bordered popup of `width` columns.
fn wrapped_rows(text: &str, width: u16) -> u16 {
    text.split('\n')
        .map(|line| (super::text_width(line) as u16).div_ceil(width - 2).max(1))
        .sum()
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

pub fn render_picker(frame: &mut Frame, app: &App, title: &str, selection: &Selection, area: Rect) {
    let theme = &app.theme;
    let longest = selection
        .items
        .iter()
        .map(|item| super::text_width(item))
        .max();
    let width = (longest.unwrap_or(0) as u16 + 4).max(36);
    let area = centered(area, width, selection.items.len() as u16 + 2);

    let block = Block::bordered()
        .title(title.to_string())
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    let rows = block.inner(area);
    let list = List::new(selection.items.iter().map(String::as_str))
        .block(block)
        .highlight_style(Style::new().bg(theme.selection_bg).fg(theme.selection_fg))
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(selection.selected));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
    app.areas.borrow_mut().picker = Some(PickerHit {
        rows,
        offset: state.offset(),
        len: selection.items.len(),
        selected: selection.selected,
    });
}

pub fn render_confirm(frame: &mut Frame, app: &App, prompt: &str, area: Rect) {
    const WIDTH: u16 = 60;
    let theme = &app.theme;
    let text = format!("{prompt}\n\ny / enter: yes    n / esc: no");
    let lines = wrapped_rows(&text, WIDTH);
    let area = centered(area, WIDTH, lines + 2);

    let block = Block::bordered()
        .title(" Confirm ")
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    let confirm = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(confirm, area);
}

const CURSOR: char = '▏';

/// `before` and `after` the cursor split into rows of at most `width` columns, and the row the
/// cursor is in.
fn input_rows(before: &str, after: &str, width: usize) -> (Vec<String>, usize) {
    let mut rows = vec![String::new()];
    let mut used = 0;
    let mut cursor_row = 0;
    let chars = before
        .chars()
        .map(|c| (c, false))
        .chain([(CURSOR, true)])
        .chain(after.chars().map(|c| (c, false)));
    for (c, is_cursor) in chars {
        if c == '\n' {
            rows.push(String::new());
            used = 0;
            continue;
        }
        let char_width = c.width().unwrap_or(0);
        if used + char_width > width && used > 0 {
            rows.push(String::new());
            used = 0;
        }
        if is_cursor {
            cursor_row = rows.len() - 1;
        }
        rows.last_mut().expect("a row").push(c);
        used += char_width;
    }
    (rows, cursor_row)
}

/// First row to show so the cursor row is the last one at most.
fn scroll_to_cursor(cursor_row: usize, height: usize) -> u16 {
    (cursor_row + 1).saturating_sub(height) as u16
}

pub fn render_text_input(frame: &mut Frame, app: &App, title: &str, input: &TextInput, area: Rect) {
    const WIDTH: u16 = 60;
    let theme = &app.theme;
    let (before, after) = input.split();
    let (rows, cursor_row) = input_rows(before, after, usize::from(WIDTH - 2));
    let height = rows
        .len()
        .min(usize::from(area.height.saturating_sub(4)).max(1));
    let scroll = scroll_to_cursor(cursor_row, height);
    let area = centered(area, WIDTH, height as u16 + 2);

    let block = Block::bordered()
        .title(title.to_string())
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    let lines: Vec<Line> = rows.into_iter().map(Line::from).collect();
    let input = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(Clear, area);
    frame.render_widget(input, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_wrap_by_display_width_and_find_the_cursor_row() {
        let (rows, cursor_row) = input_rows("日本語a", "b\nc", 4);

        assert_eq!(rows, ["日本", "語a▏", "b", "c"]);
        assert_eq!(cursor_row, 1);
    }

    #[test]
    fn scroll_keeps_the_cursor_row_visible_in_a_100_line_text() {
        let text = vec!["line"; 100].join("\n");
        let (rows, cursor_row) = input_rows(&text, "", 58);
        assert_eq!(rows.len(), 100);

        for height in [1, 10, 40] {
            let scroll = usize::from(scroll_to_cursor(cursor_row, height));
            assert!((scroll..scroll + height).contains(&cursor_row));
        }
        assert_eq!(scroll_to_cursor(0, 10), 0);
        assert_eq!(scroll_to_cursor(5, 10), 0);
        assert_eq!(scroll_to_cursor(10, 10), 1);
    }
}
