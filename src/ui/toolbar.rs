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
    if let Some(notice) = app.notice() {
        spans.push(Span::styled(
            format!("  {notice}"),
            Style::new().fg(theme.build_succeeded),
        ));
    }
    let updated = app
        .last_updated
        .map(|at| format!("updated {} · ", ago(at.elapsed().as_secs())))
        .unwrap_or_default();
    let user = app
        .user
        .as_ref()
        .map(|user| format!("{updated}{} ", user.provider_display_name))
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

fn ago(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s ago"),
        60..3600 => format!("{}m ago", secs / 60),
        _ => format!("{}h ago", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_uses_largest_whole_unit() {
        assert_eq!(ago(0), "0s ago");
        assert_eq!(ago(59), "59s ago");
        assert_eq!(ago(60), "1m ago");
        assert_eq!(ago(3599), "59m ago");
        assert_eq!(ago(7200), "2h ago");
    }
}
