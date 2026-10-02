mod setup;
mod toolbar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::app::{App, LEFT_PANELS, Panel, Screen};

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
    let block = Block::bordered()
        .title(format!(" {} ", panel.title()))
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(border));
    frame.render_widget(block, area);
}
