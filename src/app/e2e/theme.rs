//! `theme` picks the palette the whole screen is drawn with.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use super::{AdoClient, App, Auth, Harness, MockAdo, SCREEN_HEIGHT, SCREEN_WIDTH, config};
use crate::theme::ThemeName;

async fn connected(theme: ThemeName) -> Harness {
    let mut config = config();
    config.theme = theme;
    let ado = MockAdo::start().await;
    let mut app = App::new(Some(config), None);
    app.client = Some(AdoClient::with_base_url(
        Auth::from_pat("e2e-pat"),
        &ado.uri(),
    ));
    app.sync_api();
    let mut harness = Harness { app, ado };
    harness.connect().await;
    harness
}

/// Background of the top-left cell and of a cell on the toolbar row.
fn backgrounds(harness: &Harness) -> (Color, Color) {
    let mut terminal =
        Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
    terminal
        .draw(|frame| crate::ui::render(frame, &harness.app))
        .expect("render");
    let buffer = terminal.backend().buffer();
    (buffer[(0, 0)].bg, buffer[(0, SCREEN_HEIGHT - 1)].bg)
}

#[tokio::test]
async fn terminal_theme_keeps_the_terminal_background() {
    let harness = connected(ThemeName::Terminal).await;

    assert_eq!(backgrounds(&harness), (Color::Reset, Color::Reset));
    assert!(harness.screen().contains("Inbox"));
}

#[tokio::test]
async fn light_theme_draws_the_latte_background() {
    let harness = connected(ThemeName::Light).await;

    let light = Color::Rgb(0xef, 0xf1, 0xf5);
    assert_eq!(backgrounds(&harness).0, light);
    assert!(harness.screen().contains("Inbox"));
}

#[tokio::test]
async fn default_theme_is_mocha() {
    let harness = connected(ThemeName::Mocha).await;

    assert_eq!(backgrounds(&harness).0, Color::Rgb(0x1e, 0x1e, 0x2e));
}
