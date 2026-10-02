mod api;
mod app;
mod auth;
mod config;
mod images;
mod theme;
mod ui;

#[cfg(test)]
mod test_support;

use std::fs::{self, File};
use std::io::stdout;
use std::sync::Mutex;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use config::Config;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;
    let config = Config::load()?;
    let mut terminal = ratatui::init();
    // Must run before the event stream starts reading stdin.
    let picker = config
        .as_ref()
        .is_none_or(|config| config.show_images)
        .then(ratatui_image::picker::Picker::from_query_stdio)
        .and_then(|picker| {
            picker
                .inspect_err(|err| tracing::info!("no terminal graphics: {err}"))
                .ok()
        });
    // Needed to tell ctrl+h/ctrl+j apart from backspace/enter.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false)
        && execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
    let result = app::App::new(config, picker).run(&mut terminal).await;
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    result
}

/// Logs to `$XDG_STATE_HOME/azuredevopstui/log` (`%LOCALAPPDATA%` on Windows); level from `RUST_LOG`, default `info`.
fn init_logging() -> anyhow::Result<()> {
    let Some(dir) = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|dir| dir.join("azuredevopstui"))
    else {
        return Ok(());
    };
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let file = File::create(dir.join("log")).context("failed to create log file")?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .init();
    Ok(())
}
