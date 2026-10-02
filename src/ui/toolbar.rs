use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let spans: Vec<Span> = app
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
    frame.render_widget(
        Line::from(spans).style(Style::new().bg(theme.toolbar_bg)),
        area,
    );
}
