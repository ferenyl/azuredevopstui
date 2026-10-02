mod toolbar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::app::{App, LEFT_PANELS, Panel};

pub fn render(frame: &mut Frame, app: &App) {
    let theme = &app.config.colors;
    frame.render_widget(
        Block::new().style(Style::new().bg(theme.background).fg(theme.foreground)),
        frame.area(),
    );
    let [main, toolbar_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(25), Constraint::Percentage(75)]).areas(main);
    let left_areas = Layout::vertical([Constraint::Ratio(1, 4); 4]).split(left);

    for (panel, area) in LEFT_PANELS.iter().zip(left_areas.iter()) {
        render_panel(frame, app, *panel, *area);
    }
    render_panel(frame, app, Panel::Detail, right);
    toolbar::render(frame, app, toolbar_area);
}

fn render_panel(frame: &mut Frame, app: &App, panel: Panel, area: Rect) {
    let theme = &app.config.colors;
    let border = if app.focus == panel {
        theme.border_focused
    } else {
        theme.border
    };
    let block = Block::bordered()
        .title(format!(" {} ", panel.title()))
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(border));
    frame.render_widget(block, area);
}
