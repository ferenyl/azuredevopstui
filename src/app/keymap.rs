use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Action;

/// Keys that are not configurable: `ctrl+c` always quits, `space` ticks rows in popups.
const RESERVED: [(KeyBinding, Action); 2] = [
    (
        KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        Action::Quit,
    ),
    (
        KeyBinding::new(KeyCode::Char(' '), KeyModifiers::NONE),
        Action::Toggle,
    ),
];

/// A key with its modifiers, as written in the config (`"q"`, `"G"`, `"ctrl+d"`, `"shift+tab"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Shift is part of the character (`G`, `?`), so it is dropped for characters and `shift+tab`.
    const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let mut modifiers = modifiers.intersection(
            KeyModifiers::CONTROL
                .union(KeyModifiers::ALT)
                .union(KeyModifiers::SHIFT),
        );
        let code = match code {
            KeyCode::Char(c) if modifiers.contains(KeyModifiers::SHIFT) => {
                KeyCode::Char(c.to_ascii_uppercase())
            }
            _ => code,
        };
        if matches!(code, KeyCode::Char(_) | KeyCode::BackTab) {
            modifiers = modifiers.difference(KeyModifiers::SHIFT);
        }
        Self { code, modifiers }
    }

    /// Modifier prefix like `ctrl+`, empty without modifiers.
    fn prefix(self) -> String {
        [
            (KeyModifiers::CONTROL, "ctrl+"),
            (KeyModifiers::ALT, "alt+"),
            (KeyModifiers::SHIFT, "shift+"),
        ]
        .into_iter()
        .filter(|(modifier, _)| self.modifiers.contains(*modifier))
        .map(|(_, text)| text)
        .collect()
    }

    fn key_name(self) -> String {
        match self.code {
            KeyCode::Char(' ') => "space".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("f{n}"),
            code => NAMES
                .iter()
                .find(|(_, named)| *named == code)
                .map_or("?", |(name, _)| name)
                .into(),
        }
    }
}

impl From<KeyEvent> for KeyBinding {
    fn from(key: KeyEvent) -> Self {
        Self::new(key.code, key.modifiers)
    }
}

/// Names of keys other than characters; the first name of a key is the one shown.
const NAMES: [(&str, KeyCode); 23] = [
    ("enter", KeyCode::Enter),
    ("esc", KeyCode::Esc),
    ("tab", KeyCode::Tab),
    ("shift+tab", KeyCode::BackTab),
    ("backtab", KeyCode::BackTab),
    ("backspace", KeyCode::Backspace),
    ("delete", KeyCode::Delete),
    ("del", KeyCode::Delete),
    ("insert", KeyCode::Insert),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pgup", KeyCode::PageUp),
    ("pageup", KeyCode::PageUp),
    ("pgdn", KeyCode::PageDown),
    ("pagedown", KeyCode::PageDown),
    ("↑", KeyCode::Up),
    ("up", KeyCode::Up),
    ("↓", KeyCode::Down),
    ("down", KeyCode::Down),
    ("←", KeyCode::Left),
    ("left", KeyCode::Left),
    ("→", KeyCode::Right),
    ("right", KeyCode::Right),
];

impl FromStr for KeyBinding {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        if text.eq_ignore_ascii_case("shift+tab") {
            return Ok(Self::new(KeyCode::BackTab, KeyModifiers::NONE));
        }
        let (prefix, key) = match text {
            "+" => ("", "+"),
            _ if text.ends_with("++") => (&text[..text.len() - 1], "+"),
            _ => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let mut modifiers = KeyModifiers::NONE;
        for modifier in prefix.split('+').filter(|m| !m.is_empty()) {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return Err(()),
            };
        }
        let mut chars = key.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => NAMES
                .iter()
                .find(|(name, _)| *name == key)
                .map_or(KeyCode::Char(c), |(_, code)| *code),
            (None, _) => return Err(()),
            _ if key.eq_ignore_ascii_case("space") => KeyCode::Char(' '),
            _ => match key
                .to_ascii_lowercase()
                .strip_prefix('f')
                .and_then(|n| n.parse::<u8>().ok())
            {
                Some(n) if (1..=24).contains(&n) => KeyCode::F(n),
                _ => NAMES
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(key))
                    .map(|(_, code)| *code)
                    .ok_or(())?,
            },
        };
        Ok(Self::new(code, modifiers))
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.prefix(), self.key_name())
    }
}

/// Which keys trigger which action: the defaults, with the actions listed in the config's `keys`
/// replaced.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<(Action, Vec<KeyBinding>)>,
    by_key: HashMap<KeyBinding, Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(&BTreeMap::new()).expect("default keys are valid")
    }
}

impl Keymap {
    /// Builds the keymap from the config's `keys` (action name → keys).
    pub fn new(keys: &BTreeMap<String, Vec<String>>) -> Result<Self> {
        let mut bindings: Vec<(Action, Vec<KeyBinding>)> = Action::ALL
            .into_iter()
            .map(|action| (action, defaults(action)))
            .collect();
        for (name, texts) in keys {
            let Some((_, keys)) = bindings.iter_mut().find(|(a, _)| a.name() == name) else {
                let valid: Vec<_> = Action::ALL.iter().map(|a| a.name()).collect();
                bail!(
                    "unknown action \"{name}\" in keys; valid: {}",
                    valid.join(", ")
                );
            };
            *keys = Vec::new();
            for text in texts {
                let key: KeyBinding = text
                    .parse()
                    .map_err(|()| anyhow!("invalid key \"{text}\" for action \"{name}\""))?;
                if let Some((_, owner)) = RESERVED.iter().find(|(reserved, _)| *reserved == key) {
                    bail!(
                        "key \"{text}\" for action \"{name}\" is reserved for {}",
                        owner.name()
                    );
                }
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        bindings.push((Action::Toggle, vec![RESERVED[1].0]));
        let mut by_key = HashMap::new();
        for (action, keys) in &bindings {
            for key in keys {
                if let Some(other) = by_key.insert(*key, *action) {
                    bail!(
                        "key \"{key}\" is bound to both \"{}\" and \"{}\" in keys",
                        other.name(),
                        action.name()
                    );
                }
            }
        }
        Ok(Self { bindings, by_key })
    }

    /// The action of a pressed key; `ctrl+c` always quits.
    pub fn action(&self, key: KeyEvent) -> Option<Action> {
        let key = KeyBinding::from(key);
        if key == RESERVED[0].0 {
            return Some(Action::Quit);
        }
        self.by_key
            .get(&key)
            .or_else(|| {
                self.by_key
                    .get(&KeyBinding::new(key.code, KeyModifiers::NONE))
            })
            .copied()
    }

    pub fn keys(&self, action: Action) -> &[KeyBinding] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, keys)| keys)
    }

    /// Keys shown for `action`, joined by `/`: all its keys, or the first key of each action in its
    /// group (`j/k`). A shared modifier is written once (`ctrl+d/u`). Empty when nothing is bound.
    pub fn label(&self, action: Action) -> String {
        let keys: Vec<KeyBinding> = match action.group() {
            Some(group) => group
                .iter()
                .filter_map(|a| self.keys(*a).first().copied())
                .collect(),
            None => self.keys(action).to_vec(),
        };
        let Some(first) = keys.first() else {
            return String::new();
        };
        let prefix = first.prefix();
        if keys.len() > 1 && !prefix.is_empty() && keys.iter().all(|k| k.prefix() == prefix) {
            let names: Vec<_> = keys.iter().map(|k| k.key_name()).collect();
            return format!("{prefix}{}", names.join("/"));
        }
        let names: Vec<_> = keys.iter().map(ToString::to_string).collect();
        names.join("/")
    }
}

fn defaults(action: Action) -> Vec<KeyBinding> {
    let texts: &[&str] = match action {
        Action::Quit => &["q"],
        Action::Cancel => &["esc"],
        Action::Up => &["k", "up"],
        Action::Down => &["j", "down"],
        Action::Confirm => &["enter"],
        Action::Reload => &["r"],
        Action::FocusLeft => &["ctrl+h", "ctrl+left"],
        Action::FocusRight => &["ctrl+l", "ctrl+right"],
        Action::FocusUp => &["ctrl+k", "ctrl+up"],
        Action::FocusDown => &["ctrl+j", "ctrl+down"],
        Action::OpenInBrowser => &["o"],
        Action::Copy => &["y"],
        Action::CopyLink => &["Y"],
        Action::AssignToMe => &["a"],
        Action::Unassign => &["u"],
        Action::Tag => &["t"],
        Action::Comment => &["m"],
        Action::ChangeColumn => &["s"],
        Action::Complete => &["c"],
        Action::ChangeToken => &["T"],
        Action::Sort => &["S"],
        Action::Filter => &["f"],
        Action::Sprint => &["i"],
        Action::Toggle => &["space"],
        Action::NextTab => &["tab"],
        Action::PrevTab => &["shift+tab"],
        Action::Help => &["?"],
        Action::NextLink => &["l"],
        Action::PrevLink => &["L"],
        Action::Vote => &["v"],
        Action::NextThread => &["]"],
        Action::PrevThread => &["["],
        Action::ThreadStatus => &["x"],
        Action::PrActions => &["e"],
        Action::SwitchProject => &["P"],
        Action::Zoom => &["z"],
        Action::Top => &["g"],
        Action::Bottom => &["G"],
        Action::PageDown => &["pgdn"],
        Action::PageUp => &["pgup"],
        Action::HalfPageDown => &["ctrl+d"],
        Action::HalfPageUp => &["ctrl+u"],
        Action::JumpToPanel(n) => &[["1", "2", "3", "4", "5", "6"][usize::from(n - 1)]],
        Action::Search => &["/"],
        Action::Errors => &["E"],
        Action::NewWorkItem => &["n"],
    };
    texts
        .iter()
        .map(|text| text.parse().expect("default key parses"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        Keymap::default().action(KeyEvent::new(code, modifiers))
    }

    fn plain(code: KeyCode) -> Option<Action> {
        key(code, KeyModifiers::NONE)
    }

    fn keymap(json: serde_json::Value) -> Result<Keymap> {
        Keymap::new(&serde_json::from_value(json).unwrap())
    }

    fn error(json: serde_json::Value) -> String {
        keymap(json).unwrap_err().to_string()
    }

    #[test]
    fn hjkl_and_arrows_move() {
        assert_eq!(plain(KeyCode::Char('j')), Some(Action::Down));
        assert_eq!(plain(KeyCode::Down), Some(Action::Down));
        assert_eq!(plain(KeyCode::Char('k')), Some(Action::Up));
        assert_eq!(plain(KeyCode::Up), Some(Action::Up));
    }

    #[test]
    fn ctrl_direction_changes_focus() {
        let ctrl = KeyModifiers::CONTROL;

        assert_eq!(key(KeyCode::Char('h'), ctrl), Some(Action::FocusLeft));
        assert_eq!(key(KeyCode::Char('l'), ctrl), Some(Action::FocusRight));
        assert_eq!(key(KeyCode::Char('k'), ctrl), Some(Action::FocusUp));
        assert_eq!(key(KeyCode::Char('j'), ctrl), Some(Action::FocusDown));
        assert_eq!(key(KeyCode::Left, ctrl), Some(Action::FocusLeft));
        assert_eq!(key(KeyCode::Right, ctrl), Some(Action::FocusRight));
        assert_eq!(key(KeyCode::Up, ctrl), Some(Action::FocusUp));
        assert_eq!(key(KeyCode::Down, ctrl), Some(Action::FocusDown));
    }

    #[test]
    fn unbound_modifiers_fall_back_to_the_plain_key() {
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;

        assert_eq!(key(KeyCode::Char('['), altgr), Some(Action::PrevThread));
        assert_eq!(key(KeyCode::Char(']'), altgr), Some(Action::NextThread));
        assert_eq!(key(KeyCode::Down, KeyModifiers::SHIFT), Some(Action::Down));
        assert_eq!(
            key(KeyCode::Enter, KeyModifiers::CONTROL),
            Some(Action::Confirm)
        );
    }

    #[test]
    fn lowercase_and_uppercase_s_differ() {
        assert_eq!(plain(KeyCode::Char('s')), Some(Action::ChangeColumn));
        assert_eq!(
            key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            Some(Action::Sort)
        );
        assert_eq!(plain(KeyCode::Char('S')), Some(Action::Sort));
        assert_eq!(
            key(KeyCode::Char('s'), KeyModifiers::SHIFT),
            Some(Action::Sort)
        );
    }

    #[test]
    fn tab_and_back_tab_switch_tabs() {
        assert_eq!(plain(KeyCode::Tab), Some(Action::NextTab));
        assert_eq!(
            key(KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(Action::PrevTab)
        );
        assert_eq!(plain(KeyCode::BackTab), Some(Action::PrevTab));
    }

    #[test]
    fn quit_keys() {
        assert_eq!(plain(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Action::Quit)
        );
    }

    #[test]
    fn other_keys() {
        assert_eq!(plain(KeyCode::Esc), Some(Action::Cancel));
        assert_eq!(plain(KeyCode::Enter), Some(Action::Confirm));
        assert_eq!(
            key(KeyCode::Char('?'), KeyModifiers::SHIFT),
            Some(Action::Help)
        );
        assert_eq!(plain(KeyCode::Char(' ')), Some(Action::Toggle));
        assert_eq!(plain(KeyCode::Char('f')), Some(Action::Filter));
        assert_eq!(plain(KeyCode::Char('z')), Some(Action::Zoom));
        assert_eq!(plain(KeyCode::Char('/')), Some(Action::Search));
        assert_eq!(
            key(KeyCode::Char('E'), KeyModifiers::SHIFT),
            Some(Action::Errors)
        );
    }

    #[test]
    fn unbound_key_is_ignored() {
        assert_eq!(plain(KeyCode::Char('Z')), None);
        assert_eq!(plain(KeyCode::F(1)), None);
        assert_eq!(plain(KeyCode::Char('0')), None);
        assert_eq!(plain(KeyCode::Char('7')), None);
    }

    #[test]
    fn navigation_keys() {
        let ctrl = KeyModifiers::CONTROL;

        assert_eq!(plain(KeyCode::Char('g')), Some(Action::Top));
        assert_eq!(
            key(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Action::Bottom)
        );
        assert_eq!(plain(KeyCode::PageDown), Some(Action::PageDown));
        assert_eq!(plain(KeyCode::PageUp), Some(Action::PageUp));
        assert_eq!(key(KeyCode::Char('d'), ctrl), Some(Action::HalfPageDown));
        assert_eq!(key(KeyCode::Char('u'), ctrl), Some(Action::HalfPageUp));
        assert_eq!(plain(KeyCode::Char('u')), Some(Action::Unassign));
        assert_eq!(plain(KeyCode::Char('1')), Some(Action::JumpToPanel(1)));
        assert_eq!(plain(KeyCode::Char('6')), Some(Action::JumpToPanel(6)));
    }

    #[test]
    fn keys_print_and_parse_back() {
        for text in [
            "q",
            "G",
            "?",
            "/",
            "1",
            "ctrl+d",
            "alt+x",
            "shift+tab",
            "enter",
            "esc",
            "space",
            "pgdn",
            "pgup",
            "↑",
            "ctrl+←",
            "f5",
            "+",
            "ctrl++",
            "shift+↓",
        ] {
            let key: KeyBinding = text.parse().unwrap();
            assert_eq!(key.to_string(), text);
            assert_eq!(key.to_string().parse::<KeyBinding>(), Ok(key));
        }
    }

    #[test]
    fn key_names_are_lenient_and_shift_makes_uppercase() {
        let parse = |text: &str| text.parse::<KeyBinding>().unwrap().to_string();

        assert_eq!(parse("up"), "↑");
        assert_eq!(parse("Ctrl+Left"), "ctrl+←");
        assert_eq!(parse("PageDown"), "pgdn");
        assert_eq!(parse("backtab"), "shift+tab");
        assert_eq!(parse("shift+g"), "G");
        assert_eq!(parse("shift+?"), "?");
    }

    #[test]
    fn invalid_keys_do_not_parse() {
        for text in ["", "ctrl+", "hyper+a", "ab", "f0", "f25", "enterr"] {
            assert!(text.parse::<KeyBinding>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn override_replaces_only_that_action() {
        let keymap = keymap(serde_json::json!({ "vote": ["V", "ctrl+v"] })).unwrap();
        let press = |code, modifiers| keymap.action(KeyEvent::new(code, modifiers));

        assert_eq!(
            press(KeyCode::Char('V'), KeyModifiers::SHIFT),
            Some(Action::Vote)
        );
        assert_eq!(
            press(KeyCode::Char('v'), KeyModifiers::CONTROL),
            Some(Action::Vote)
        );
        assert_eq!(press(KeyCode::Char('v'), KeyModifiers::NONE), None);
        assert_eq!(
            press(KeyCode::Char('m'), KeyModifiers::NONE),
            Some(Action::Comment)
        );
        assert_eq!(keymap.label(Action::Vote), "V/ctrl+v");
    }

    #[test]
    fn empty_list_unbinds_the_action() {
        let keymap = keymap(serde_json::json!({ "zoom": [] })).unwrap();

        assert_eq!(keymap.action(KeyEvent::from(KeyCode::Char('z'))), None);
        assert_eq!(keymap.label(Action::Zoom), "");
    }

    #[test]
    fn a_freed_default_key_can_be_reused() {
        let keymap = keymap(serde_json::json!({ "vote": ["V"], "comment": ["v"] })).unwrap();

        assert_eq!(
            keymap.action(KeyEvent::from(KeyCode::Char('v'))),
            Some(Action::Comment)
        );
    }

    #[test]
    fn unknown_action_names_the_valid_ones() {
        let message = error(serde_json::json!({ "vot": ["v"] }));

        assert!(
            message.starts_with("unknown action \"vot\" in keys; valid: quit, cancel, up,"),
            "{message}"
        );
        assert!(message.contains("panel_6"), "{message}");
    }

    #[test]
    fn invalid_key_names_the_action() {
        assert_eq!(
            error(serde_json::json!({ "vote": ["ctrl+"] })),
            "invalid key \"ctrl+\" for action \"vote\""
        );
    }

    #[test]
    fn key_bound_twice_names_both_actions() {
        assert_eq!(
            error(serde_json::json!({ "vote": ["m"] })),
            "key \"m\" is bound to both \"comment\" and \"vote\" in keys"
        );
        assert_eq!(
            error(serde_json::json!({ "vote": ["Z"], "zoom": ["Z"] })),
            "key \"Z\" is bound to both \"vote\" and \"zoom\" in keys"
        );
    }

    #[test]
    fn reserved_keys_cannot_be_bound() {
        assert_eq!(
            error(serde_json::json!({ "vote": ["ctrl+c"] })),
            "key \"ctrl+c\" for action \"vote\" is reserved for quit"
        );
        assert_eq!(
            error(serde_json::json!({ "vote": ["space"] })),
            "key \"space\" for action \"vote\" is reserved for toggle"
        );
    }

    #[test]
    fn ctrl_c_quits_even_when_q_is_rebound() {
        let keymap = keymap(serde_json::json!({ "quit": [] })).unwrap();

        assert_eq!(
            keymap.action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn labels_join_keys_and_groups() {
        let keymap = Keymap::default();

        assert_eq!(keymap.label(Action::Down), "j/k");
        assert_eq!(keymap.label(Action::Up), "k/↑");
        assert_eq!(keymap.label(Action::FocusRight), "ctrl+h/j/k/l");
        assert_eq!(keymap.label(Action::FocusLeft), "ctrl+h/←");
        assert_eq!(keymap.label(Action::HalfPageDown), "ctrl+d/u");
        assert_eq!(keymap.label(Action::Top), "g/G");
        assert_eq!(keymap.label(Action::PageDown), "pgdn/pgup");
        assert_eq!(keymap.label(Action::JumpToPanel(1)), "1/2/3/4/5/6");
        assert_eq!(keymap.label(Action::Toggle), "space");
        assert_eq!(keymap.label(Action::PrevTab), "shift+tab");
    }

    #[test]
    fn every_action_has_a_default_key() {
        let keymap = Keymap::default();

        for action in Action::ALL {
            assert!(!keymap.keys(action).is_empty(), "{action:?}");
        }
    }

    #[test]
    fn readme_lists_every_action_name() {
        let readme = include_str!("../../README.md");

        for action in Action::ALL {
            assert!(
                readme.contains(&format!("| `{}` |", action.name())),
                "{} missing in README",
                action.name()
            );
        }
    }
}
