use super::TextInput;
use crate::config::Config;

pub enum SetupStep {
    Loading(&'static str),
    EnterPat {
        input: TextInput,
        error: Option<String>,
        /// Opened from the main view; cancel returns there.
        cancelable: bool,
    },
    EnterOrganization(TextInput),
    SelectOrganization(Selection),
    SelectProject {
        organization: String,
        selection: Selection,
    },
    SelectTeam {
        organization: String,
        project: String,
        selection: Selection,
    },
    /// Last step; `config` is saved once a column is picked.
    SelectReadyColumn {
        config: Box<Config>,
        selection: Selection,
    },
    Failed {
        error: String,
        retry: Retry,
    },
}

pub enum Retry {
    Authenticate,
    Continue,
    Projects(String),
    Teams(String, String),
    BoardColumns(Box<Config>),
    SaveConfig(Box<Config>),
}

impl SetupStep {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Loading(_) => "Setup",
            Self::EnterPat { .. } => "Enter personal access token",
            Self::EnterOrganization(_) => "Enter organization name or URL",
            Self::SelectOrganization(_) => "Select organization",
            Self::SelectProject { .. } => "Select project",
            Self::SelectTeam { .. } => "Select team",
            Self::SelectReadyColumn { .. } => "Select ready column",
            Self::Failed { .. } => "Setup failed",
        }
    }

    pub fn selection(&self) -> Option<&Selection> {
        match self {
            Self::SelectOrganization(selection)
            | Self::SelectProject { selection, .. }
            | Self::SelectTeam { selection, .. }
            | Self::SelectReadyColumn { selection, .. } => Some(selection),
            _ => None,
        }
    }

    pub fn selection_mut(&mut self) -> Option<&mut Selection> {
        match self {
            Self::SelectOrganization(selection)
            | Self::SelectProject { selection, .. }
            | Self::SelectTeam { selection, .. }
            | Self::SelectReadyColumn { selection, .. } => Some(selection),
            _ => None,
        }
    }

    pub fn input_mut(&mut self) -> Option<&mut TextInput> {
        match self {
            Self::EnterPat { input, .. } | Self::EnterOrganization(input) => Some(input),
            _ => None,
        }
    }

    pub fn is_input(&self) -> bool {
        matches!(self, Self::EnterPat { .. } | Self::EnterOrganization(_))
    }
}

/// Organization name from `name`, `https://dev.azure.com/name/...` or `https://name.visualstudio.com`.
pub fn organization_name(input: &str) -> &str {
    let input = input.trim();
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let mut segments = rest.split('/');
    let host = segments.next().unwrap_or_default();
    if host.eq_ignore_ascii_case("dev.azure.com") {
        segments.next().unwrap_or_default()
    } else if let Some(name) = host.strip_suffix(".visualstudio.com") {
        name
    } else {
        host
    }
}

pub struct Selection {
    pub items: Vec<String>,
    /// Index into the visible items, which are all of them without a query.
    pub selected: usize,
    /// Only items containing this are visible, ignoring case.
    pub query: Option<TextInput>,
}

impl Selection {
    pub fn new(items: Vec<String>) -> Self {
        Self {
            items,
            selected: 0,
            query: None,
        }
    }

    /// Selects `item` if listed.
    pub fn preselect(&mut self, item: &str) {
        if let Some(index) = self.visible().iter().position(|i| *i == item) {
            self.selected = index;
        }
    }

    /// The items matching the query.
    pub fn visible(&self) -> Vec<&str> {
        let query = self.query.as_ref().map(|q| q.as_str().to_lowercase());
        self.items
            .iter()
            .map(String::as_str)
            .filter(|item| {
                query
                    .as_deref()
                    .is_none_or(|query| item.to_lowercase().contains(query))
            })
            .collect()
    }

    pub fn next(&mut self) {
        if self.selected + 1 < self.visible().len() {
            self.selected += 1;
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn current(&self) -> Option<&str> {
        self.visible().get(self.selected).copied()
    }

    pub fn start_query(&mut self) {
        self.query.get_or_insert_with(TextInput::default);
    }

    /// Shows everything again, keeping the current item selected.
    pub fn clear_query(&mut self) {
        let current = self.current().map(str::to_string);
        self.query = None;
        if let Some(current) = current {
            self.preselect(&current);
        }
    }

    /// Edits the query with the key; true when it was consumed.
    pub fn edit_query(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let Some(query) = &mut self.query else {
            return false;
        };
        let edited = query.handle_key(key, false);
        if edited {
            self.selected = 0;
        }
        edited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(items: &[&str]) -> Selection {
        Selection::new(items.iter().map(|item| item.to_string()).collect())
    }

    #[test]
    fn selection_stops_at_the_ends() {
        let mut selection = selection(&["a", "b"]);

        selection.previous();
        assert_eq!(selection.current(), Some("a"));
        selection.next();
        selection.next();
        assert_eq!(selection.current(), Some("b"));
    }

    #[test]
    fn preselect_picks_a_listed_item_only() {
        let mut selection = selection(&["a", "b", "c"]);

        selection.preselect("c");
        assert_eq!(selection.current(), Some("c"));
        selection.preselect("missing");
        assert_eq!(selection.current(), Some("c"));
    }

    #[test]
    fn query_shows_matching_items_ignoring_case_and_moves_within_them() {
        let mut selection = selection(&["Fabrikam", "Northwind", "fabric"]);
        selection.selected = 2;
        selection.query = Some(TextInput::new("FAB"));

        assert_eq!(selection.visible(), ["Fabrikam", "fabric"]);
        selection.selected = 0;
        selection.next();
        assert_eq!(selection.current(), Some("fabric"));
        selection.next();
        assert_eq!(selection.current(), Some("fabric"));
        selection.previous();
        assert_eq!(selection.current(), Some("Fabrikam"));
    }

    #[test]
    fn editing_the_query_resets_the_selection_and_clearing_keeps_the_item() {
        let mut selection = selection(&["a1", "b1", "b2"]);
        selection.start_query();
        selection.selected = 1;

        let key = |c| crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(c));
        assert!(selection.edit_query(key('b')));
        assert_eq!(selection.selected, 0);
        selection.next();
        assert_eq!(selection.current(), Some("b2"));
        selection.clear_query();

        assert!(selection.query.is_none());
        assert_eq!(selection.current(), Some("b2"));
        assert!(!selection.edit_query(key('x')));
    }

    #[test]
    fn query_without_matches_has_no_current() {
        let mut selection = selection(&["a"]);
        selection.query = Some(TextInput::new("z"));

        assert_eq!(selection.current(), None);
    }

    #[test]
    fn empty_selection_has_no_current() {
        let mut selection = selection(&[]);

        selection.next();

        assert_eq!(selection.selected, 0);
        assert_eq!(selection.current(), None);
    }

    #[test]
    fn input_steps() {
        let mut pat = SetupStep::EnterPat {
            input: TextInput::default(),
            error: None,
            cancelable: false,
        };
        let mut organization = SetupStep::EnterOrganization(TextInput::default());

        assert!(pat.is_input());
        assert!(organization.is_input());
        pat.input_mut().unwrap().insert('x');
        assert!(matches!(pat, SetupStep::EnterPat { input, .. } if input.as_str() == "x"));
        assert!(organization.input_mut().is_some());
        assert!(!SetupStep::Loading("Signing in…").is_input());
    }

    #[test]
    fn selection_steps() {
        let mut steps = [
            SetupStep::SelectOrganization(selection(&["o"])),
            SetupStep::SelectProject {
                organization: "o".into(),
                selection: selection(&["p"]),
            },
            SetupStep::SelectTeam {
                organization: "o".into(),
                project: "p".into(),
                selection: selection(&["t"]),
            },
            SetupStep::SelectReadyColumn {
                config: Box::new(crate::test_support::config()),
                selection: selection(&["Ready"]),
            },
        ];

        for step in &mut steps {
            assert!(step.selection().is_some(), "{}", step.title());
            assert!(step.selection_mut().is_some());
            assert!(!step.is_input());
        }
        assert!(SetupStep::Loading("…").selection().is_none());
    }
}
