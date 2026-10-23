mod api;
mod app;
mod auth;
mod config;
mod diff;
mod images;
mod notify;
mod rich_text;
mod terminal;
mod theme;
mod ui;

#[cfg(test)]
mod test_support;

use std::fs::{self, File};
use std::sync::Mutex;

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use config::Config;
use terminal::TerminalModes;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(run());
    // Notification threads may still wait for a click; don't block exit on them.
    runtime.shutdown_background();
    result
}

async fn run() -> anyhow::Result<()> {
    init_logging()?;
    if std::env::args().any(|arg| arg == "--test-notification") {
        let clicked = notify::Notifier::default()
            .send(
                "azuredevopstui".into(),
                "Test notification".into(),
                Some("open".into()),
            )
            .await;
        println!(
            "{}",
            if clicked.is_some() {
                "opened"
            } else {
                "closed"
            }
        );
        return Ok(());
    }
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
    let mut modes = TerminalModes::enable(config.as_ref().is_none_or(|config| config.mouse));
    let result = app::App::new(config, picker)
        .run(&mut terminal, &mut modes)
        .await;
    modes.disable();
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
