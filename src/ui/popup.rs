use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph};

use crate::app::{App, ColumnPicker, HELP};

const KEY_WIDTH: usize = 12;

pub fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let lines: Vec<Line> = HELP
        .iter()
        .map(|action| {
            Line::from(vec![
                Span::styled(
                    format!(" {:<KEY_WIDTH$}", action.key_label()),
                    Style::new().fg(theme.toolbar_key),
                ),
                Span::raw(action.label()),
            ])
        })
        .collect();
    let area = centered(area, 44, lines.len() as u16 + 2);
    let block = Block::bordered()
        .title(" Keys ")
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
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

pub fn render_column_picker(frame: &mut Frame, app: &App, picker: &ColumnPicker, area: Rect) {
    let theme = &app.theme;
    let area = centered(area, 36, picker.selection.items.len() as u16 + 2);

    let block = Block::bordered()
        .title(format!(" Move #{} ", picker.work_item_id))
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused))
        .style(Style::new().bg(theme.background).fg(theme.foreground));
    let list = List::new(picker.selection.items.iter().map(String::as_str))
        .block(block)
        .highlight_style(Style::new().bg(theme.selection_bg).fg(theme.selection_fg))
        .highlight_symbol("> ");
    let mut state = ListState::default().with_selected(Some(picker.selection.selected));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}
