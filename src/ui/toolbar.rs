use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let mut spans: Vec<Span> = app
        .actions()
        .iter()
        .flat_map(|action| {
            [
                Span::styled(
                    format!(" [{}] ", action.key_label()),
                    Style::new().fg(theme.toolbar_key),
                ),
                Span::raw(action.label()),
            ]
        })
        .collect();
    if let Some(error) = &app.error {
        spans.push(Span::styled(
            format!("  {error}"),
            Style::new().fg(theme.error),
        ));
    }
    let user = app
        .user
        .as_ref()
        .map(|user| format!("{} ", user.provider_display_name))
        .unwrap_or_default();
    let [actions_area, user_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(user.chars().count() as u16),
    ])
    .areas(area);

    let style = Style::new().bg(theme.toolbar_bg);
    frame.render_widget(Line::from(spans).style(style), actions_area);
    frame.render_widget(Line::from(user).style(style.fg(theme.muted)), user_area);
}
