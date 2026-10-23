use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use crate::ui::text_width;

/// Byte offset in `line` of the char at display `column`, or the line's end when it is shorter.
fn offset_at(line: &str, column: usize) -> usize {
    let mut used = 0;
    for (offset, c) in line.char_indices() {
        let width = c.width().unwrap_or(0);
        if used + width > column {
            return offset;
        }
        used += width;
    }
    line.len()
}

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

    /// Start of the cursor's line.
    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    /// End of the cursor's line.
    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    pub fn text_start(&mut self) {
        self.cursor = 0;
    }

    pub fn text_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// One line up, keeping the display column where the line is long enough.
    pub fn up(&mut self) {
        let start = self.line_start();
        if start == 0 {
            return;
        }
        let column = text_width(&self.text[start..self.cursor]);
        let previous_end = start - 1;
        let previous_start = self.text[..previous_end].rfind('\n').map_or(0, |i| i + 1);
        self.cursor = previous_start + offset_at(&self.text[previous_start..previous_end], column);
    }

    /// One line down, keeping the display column where the line is long enough.
    pub fn down(&mut self) {
        let end = self.line_end();
        if end == self.text.len() {
            return;
        }
        let column = text_width(&self.text[self.line_start()..self.cursor]);
        let next_start = end + 1;
        let next_end = self.text[next_start..]
            .find('\n')
            .map_or(self.text.len(), |i| next_start + i);
        self.cursor = next_start + offset_at(&self.text[next_start..next_end], column);
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start();
    }

    pub fn word_right(&mut self) {
        let after = &self.text[self.cursor..];
        let rest = after.trim_start();
        let word = rest.len() - rest.trim_start_matches(|c: char| !c.is_whitespace()).len();
        self.cursor += after.len() - rest.len() + word;
    }

    pub fn delete_word_back(&mut self) {
        let start = self.word_start();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }

    /// Start of the word before the cursor.
    fn word_start(&self) -> usize {
        let before = self.text[..self.cursor].trim_end();
        before.trim_end_matches(|c: char| !c.is_whitespace()).len()
    }

    /// Edits with the key; true when it was consumed. Other ctrl+character is left to the caller.
    /// With `multiline`, shift/alt+enter inserts a newline and up/down move between lines.
    pub fn handle_key(&mut self, key: KeyEvent, multiline: bool) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let word = ctrl || alt;
        match key.code {
            KeyCode::Char('w') if ctrl => self.delete_word_back(),
            KeyCode::Char('b') if alt && !ctrl => self.word_left(),
            KeyCode::Char('f') if alt && !ctrl => self.word_right(),
            KeyCode::Backspace if word => self.delete_word_back(),
            KeyCode::Left if word => self.word_left(),
            KeyCode::Right if word => self.word_right(),
            KeyCode::Home if ctrl => self.text_start(),
            KeyCode::End if ctrl => self.text_end(),
            KeyCode::Up if multiline => self.up(),
            KeyCode::Down if multiline => self.down(),
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
        assert!(!input.handle_key(key(KeyCode::Up), false));
        assert!(!input.handle_key(key(KeyCode::Down), false));

        assert_eq!(input.as_str(), "a");
        assert_eq!(input.split(), ("a", ""));
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn ctrl_backspace_and_ctrl_w_delete_a_word() {
        let mut input = TextInput::new("one two  ");

        assert!(input.handle_key(with(KeyCode::Backspace, KeyModifiers::CONTROL), false));
        assert_eq!(input.as_str(), "one ");
        assert!(input.handle_key(with(KeyCode::Char('w'), KeyModifiers::CONTROL), false));
        assert_eq!(input.as_str(), "");
        assert!(input.handle_key(with(KeyCode::Backspace, KeyModifiers::ALT), false));
        assert_eq!(input.as_str(), "");
    }

    #[test]
    fn delete_word_back_keeps_the_text_after_the_cursor() {
        let mut input = TextInput::new("one two three");
        input.left();
        input.left();
        input.left();
        input.left();
        input.left();

        input.delete_word_back();

        assert_eq!(input.as_str(), "one three");
        assert_eq!(input.split(), ("one ", "three"));
    }

    #[test]
    fn words_move_across_spaces_and_lines() {
        let mut input = TextInput::new("one two\nthree");
        input.text_start();

        input.word_right();
        assert_eq!(input.split(), ("one", " two\nthree"));
        input.word_right();
        input.word_right();
        assert_eq!(input.split(), ("one two\nthree", ""));
        input.word_left();
        assert_eq!(input.split(), ("one two\n", "three"));
        input.word_left();
        input.word_left();
        input.word_left();
        assert_eq!(input.split(), ("", "one two\nthree"));
    }

    #[test]
    fn word_keys_are_ctrl_arrows_and_alt_b_f() {
        let mut input = TextInput::new("ab cd");

        input.handle_key(with(KeyCode::Left, KeyModifiers::CONTROL), false);
        assert_eq!(input.split(), ("ab ", "cd"));
        input.handle_key(with(KeyCode::Char('b'), KeyModifiers::ALT), false);
        assert_eq!(input.split(), ("", "ab cd"));
        input.handle_key(with(KeyCode::Right, KeyModifiers::CONTROL), false);
        assert_eq!(input.split(), ("ab", " cd"));
        input.handle_key(with(KeyCode::Char('f'), KeyModifiers::ALT), false);
        assert_eq!(input.split(), ("ab cd", ""));
    }

    #[test]
    fn home_and_end_stay_on_the_line_and_ctrl_goes_to_the_text_edges() {
        let mut input = TextInput::new("ab\ncd\nef");
        input.up();

        input.home();
        assert_eq!(input.split(), ("ab\n", "cd\nef"));
        input.end();
        assert_eq!(input.split(), ("ab\ncd", "\nef"));
        input.handle_key(with(KeyCode::Home, KeyModifiers::CONTROL), true);
        assert_eq!(input.split(), ("", "ab\ncd\nef"));
        input.handle_key(with(KeyCode::End, KeyModifiers::CONTROL), true);
        assert_eq!(input.split(), ("ab\ncd\nef", ""));
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let mut input = TextInput::new("abcd\nab\nabcdef");
        input.left();
        input.left();

        input.up();
        assert_eq!(input.split(), ("abcd\nab", "\nabcdef"));
        input.up();
        assert_eq!(input.split(), ("ab", "cd\nab\nabcdef"));
        input.up();
        assert_eq!(input.split(), ("ab", "cd\nab\nabcdef"));
        input.down();
        input.down();
        assert_eq!(input.split(), ("abcd\nab\nab", "cdef"));
        input.down();
        assert_eq!(input.split(), ("abcd\nab\nab", "cdef"));
    }

    #[test]
    fn up_and_down_count_display_columns() {
        let mut input = TextInput::new("日本語\nabcdef");
        input.left();
        input.left();
        input.left();

        input.up();
        assert_eq!(input.split(), ("日", "本語\nabcdef"));
        input.down();
        assert_eq!(input.split(), ("日本語\nab", "cdef"));
        input.right();
        input.right();
        input.up();
        assert_eq!(input.split(), ("日本", "語\nabcdef"));
    }

    #[test]
    fn up_and_down_only_move_when_multiline() {
        let mut input = TextInput::new("a\nb");

        assert!(!input.handle_key(key(KeyCode::Up), false));
        assert!(input.handle_key(key(KeyCode::Up), true));
        assert_eq!(input.split(), ("a", "\nb"));
        assert!(input.handle_key(key(KeyCode::Down), true));
        assert_eq!(input.split(), ("a\nb", ""));
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
