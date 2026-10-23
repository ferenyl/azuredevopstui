use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph, Wrap};

use crate::app::{App, HELP, Selection, TextInput};

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
    let list = List::new(selection.items.iter().map(String::as_str))
        .block(block)
        .highlight_style(Style::new().bg(theme.selection_bg).fg(theme.selection_fg))
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(selection.selected));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
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

pub fn render_text_input(frame: &mut Frame, app: &App, title: &str, input: &TextInput, area: Rect) {
    const WIDTH: u16 = 60;
    let theme = &app.theme;
    let (before, after) = input.split();
    let text = format!("{before}▏{after}");
    let lines = wrapped_rows(&text, WIDTH);
    let area = centered(area, WIDTH, lines + 2);

    let block = Block::bordered()
        .title(title.to_string())
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    let input = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(input, area);
}
