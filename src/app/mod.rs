mod action;
mod state;

pub use action::Action;
pub use state::{LEFT_PANELS, Panel};

use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use ratatui::DefaultTerminal;

use crate::config::Config;

pub struct App {
    pub config: Config,
    pub focus: Panel,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            focus: Panel::MyPrs,
            should_quit: false,
        }
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        let mut events = EventStream::new();
        while !self.should_quit {
            terminal.draw(|frame| crate::ui::render(frame, &self))?;
            match events.next().await {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    self.handle_key(key)
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err.into()),
                None => break,
            }
        }
        Ok(())
    }

    pub fn actions(&self) -> &'static [Action] {
        &[Action::Quit]
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if let Some(action) = Action::from_key(key) {
            self.apply(action);
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
        }
    }
}
