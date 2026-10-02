use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListState, Paragraph, Wrap};

use crate::app::{App, SetupStep};

pub fn render(frame: &mut Frame, app: &App, step: &SetupStep, area: Rect) {
    let theme = &app.theme;
    let [area] = Layout::horizontal([Constraint::Percentage(50)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Percentage(60)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);

    let block = Block::bordered()
        .title(format!(" {} ", step.title()))
        .title_style(Style::new().fg(theme.title))
        .border_style(Style::new().fg(theme.border_focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match step {
        SetupStep::Loading(text) => {
            frame.render_widget(
                Paragraph::new(*text).style(Style::new().fg(theme.muted)),
                inner,
            );
        }
        SetupStep::EnterPat { input, error } => {
            let [input_area, error_area] =
                Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
            render_input(frame, app, input_area, &"*".repeat(input.chars().count()));
            if let Some(error) = error {
                render_error(frame, app, error_area, error);
            }
        }
        SetupStep::EnterOrganization(input) => render_input(frame, app, inner, input),
        SetupStep::Failed { error, .. } => render_error(frame, app, inner, error),
        _ => {
            if let Some(selection) = step.selection() {
                let list = List::new(selection.items.iter().map(String::as_str))
                    .highlight_style(Style::new().bg(theme.selection_bg).fg(theme.selection_fg))
                    .highlight_symbol("> ");
                let mut state = ListState::default().with_selected(Some(selection.selected));
                frame.render_stateful_widget(list, inner, &mut state);
            }
        }
    }
}

fn render_error(frame: &mut Frame, app: &App, area: Rect, error: &str) {
    frame.render_widget(
        Paragraph::new(error)
            .style(Style::new().fg(app.theme.error))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_input(frame: &mut Frame, app: &App, area: Rect, text: &str) {
    frame.render_widget(
        Line::from(format!("{text}▏")).style(Style::new().fg(app.theme.foreground)),
        area,
    );
}
