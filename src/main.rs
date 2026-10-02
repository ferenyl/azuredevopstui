mod api;
mod app;
mod auth;
mod config;
mod theme;
mod ui;

use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let mut terminal = ratatui::init();
    let result = app::App::new(config).run(&mut terminal).await;
    ratatui::restore();
    result
}
