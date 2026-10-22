use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Editable text with a cursor. `cursor` is a byte index on a char boundary.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The text before and after the cursor.
    pub fn split(&self) -> (&str, &str) {
        self.text.split_at(self.cursor)
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn backspace(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
            self.text.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
        }
    }

    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Edits with the key; true when it was consumed. Ctrl+character is left to the caller.
    /// With `multiline`, shift/alt+enter inserts a newline.
    pub fn handle_key(&mut self, key: KeyEvent, multiline: bool) -> bool {
        match key.code {
            KeyCode::Enter
                if multiline
                    && key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                self.insert('\n');
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn new_puts_cursor_at_end() {
        let input = TextInput::new("abc");

        assert_eq!(input.split(), ("abc", ""));
        assert_eq!(TextInput::default().as_str(), "");
    }

    #[test]
    fn insert_goes_at_the_cursor() {
        let mut input = TextInput::new("ac");
        input.left();

        input.insert('b');
        input.insert_str("XY");

        assert_eq!(input.as_str(), "abXYc");
        assert_eq!(input.split(), ("abXY", "c"));
    }

    #[test]
    fn backspace_and_delete_respect_the_cursor() {
        let mut input = TextInput::new("abcd");
        input.left();
        input.left();

        input.backspace();
        assert_eq!(input.as_str(), "acd");
        input.delete();
        assert_eq!(input.as_str(), "ad");
        assert_eq!(input.split(), ("a", "d"));
    }

    #[test]
    fn backspace_at_start_and_delete_at_end_do_nothing() {
        let mut input = TextInput::new("ab");
        input.delete();
        assert_eq!(input.as_str(), "ab");
        input.home();

        input.backspace();

        assert_eq!(input.as_str(), "ab");
        assert_eq!(input.split(), ("", "ab"));
    }

    #[test]
    fn movement_stops_at_the_edges() {
        let mut input = TextInput::new("ab");

        input.right();
        assert_eq!(input.split(), ("ab", ""));
        input.home();
        input.left();
        assert_eq!(input.split(), ("", "ab"));
        input.right();
        assert_eq!(input.split(), ("a", "b"));
        input.end();
        assert_eq!(input.split(), ("ab", ""));
    }

    #[test]
    fn multibyte_chars_move_and_delete_whole() {
        let mut input = TextInput::new("aåä😀z");

        input.left();
        input.left();
        assert_eq!(input.split(), ("aåä", "😀z"));
        input.backspace();
        assert_eq!(input.as_str(), "aå😀z");
        input.right();
        input.delete();
        assert_eq!(input.as_str(), "aå😀");
        input.insert('é');
        assert_eq!(input.split(), ("aå😀é", ""));
        input.home();
        input.right();
        input.right();
        assert_eq!(input.split(), ("aå", "😀é"));
    }

    #[test]
    fn handle_key_edits_and_moves() {
        let mut input = TextInput::default();

        for code in [
            KeyCode::Char('a'),
            KeyCode::Char('c'),
            KeyCode::Left,
            KeyCode::Char('b'),
            KeyCode::Home,
            KeyCode::Delete,
            KeyCode::End,
            KeyCode::Backspace,
        ] {
            assert!(input.handle_key(key(code), false));
        }

        assert_eq!(input.as_str(), "b");
    }

    #[test]
    fn ctrl_keys_and_unmapped_keys_are_not_taken() {
        let mut input = TextInput::new("a");

        assert!(!input.handle_key(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            true
        ));
        assert!(!input.handle_key(key(KeyCode::Enter), true));
        assert!(!input.handle_key(key(KeyCode::Esc), true));
        assert!(!input.handle_key(key(KeyCode::Up), true));

        assert_eq!(input.as_str(), "a");
        assert_eq!(input.split(), ("a", ""));
    }

    #[test]
    fn ctrl_backspace_still_deletes() {
        let mut input = TextInput::new("ab");

        assert!(input.handle_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
            false
        ));

        assert_eq!(input.as_str(), "a");
    }

    #[test]
    fn shift_or_alt_enter_adds_a_line_only_when_multiline() {
        let mut input = TextInput::new("a");
        let shift = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        let alt = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);

        assert!(!input.handle_key(shift, false));
        assert!(input.handle_key(shift, true));
        assert!(input.handle_key(alt, true));

        assert_eq!(input.as_str(), "a\n\n");
    }
}
