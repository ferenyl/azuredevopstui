mod api;
mod app;
mod auth;
mod config;
mod theme;
mod ui;

use std::io::stdout;

use config::Config;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let mut terminal = ratatui::init();
    // Needed to tell ctrl+h/ctrl+j apart from backspace/enter.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false)
        && execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
    let result = app::App::new(config).run(&mut terminal).await;
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    result
}
