use std::io::stdout;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};

/// Terminal modes the app switches on while it runs and off again for the shell or an editor.
pub struct TerminalModes {
    enhanced: bool,
    mouse: bool,
}

impl TerminalModes {
    /// Must run before the event stream starts reading stdin. `mouse` captures clicks and the wheel.
    pub fn enable(mouse: bool) -> Self {
        // Needed to tell ctrl+h/ctrl+j apart from backspace/enter.
        let enhanced = supports_keyboard_enhancement().unwrap_or(false)
            && execute!(
                stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .is_ok();
        // Delivers pasted text as one event instead of keystrokes.
        let _ = execute!(stdout(), EnableBracketedPaste);
        let mouse = mouse && execute!(stdout(), EnableMouseCapture).is_ok();
        Self { enhanced, mouse }
    }

    /// Hands the terminal to a child process. Unlike `ratatui::restore`/`init` this leaves the
    /// panic hook alone, so it can be repeated.
    pub fn suspend(&self) {
        self.disable();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }

    pub fn resume(&mut self) {
        let _ = enable_raw_mode();
        let _ = execute!(stdout(), EnterAlternateScreen);
        *self = Self::enable(self.mouse);
    }

    pub fn disable(&self) {
        if self.mouse {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        let _ = execute!(stdout(), DisableBracketedPaste);
        if self.enhanced {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
    }
}
