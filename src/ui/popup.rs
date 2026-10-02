use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Clear, List, ListState};

use crate::app::{App, ColumnPicker};

pub fn render_column_picker(frame: &mut Frame, app: &App, picker: &ColumnPicker, area: Rect) {
    let theme = &app.theme;
    let height = picker.selection.items.len() as u16 + 2;
    let [area] = Layout::horizontal([Constraint::Length(36)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);

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
