mod api;
mod app;
mod auth;
mod cli;
mod config;
mod diff;
mod doctor;
mod images;
mod notify;
mod rich_text;
mod terminal;
mod theme;
mod ui;

#[cfg(test)]
mod test_support;

use std::fs::{self, File};
use std::process::ExitCode;
use std::sync::Mutex;

use anyhow::Context;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

use cli::{Cli, Command};
use config::Config;
use terminal::TerminalModes;

fn main() -> anyhow::Result<ExitCode> {
    // Before logging, so `--help` and `--version` never truncate the log of a running instance.
    let cli = Cli::parse();
    if let Some(path) = &cli.config {
        Config::set_path(path.clone());
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(run(cli));
    // Notification threads may still wait for a click; don't block exit on them.
    runtime.shutdown_background();
    result
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    if cli.command == Some(Command::Doctor) {
        let (text, ok) = doctor::render(&doctor::run().await);
        print!("{text}");
        return Ok(if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    init_logging()?;
    if cli.test_notification {
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
        return Ok(ExitCode::SUCCESS);
    }
    let config = Config::load()?;
    // `--project` alone stays in the configured organization.
    let organization = cli.organization.or_else(|| {
        cli.project.as_ref()?;
        config.as_ref().map(|config| config.organization.clone())
    });
    if cli.project.is_some() && organization.is_none() {
        Cli::command()
            .error(
                ErrorKind::MissingRequiredArgument,
                "--project needs --org when there is no config",
            )
            .exit();
    }
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
    modes.restore_on_panic();
    let mut app = app::App::new(config, picker);
    if let Some(organization) = organization {
        app.start_in(organization, cli.project);
    }
    let result = app.run(&mut terminal, &mut modes).await;
    modes.disable();
    ratatui::restore();
    result.map(|()| ExitCode::SUCCESS)
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
