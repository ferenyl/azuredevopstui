use crate::config::Config;

pub enum SetupStep {
    Loading(&'static str),
    EnterPat {
        input: String,
        error: Option<String>,
        /// Opened from the main view; cancel returns there.
        cancelable: bool,
    },
    EnterOrganization(String),
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
    SelectReadyColumn(Selection),
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
            Self::SelectReadyColumn(_) => "Select ready column",
            Self::Failed { .. } => "Setup failed",
        }
    }

    pub fn selection(&self) -> Option<&Selection> {
        match self {
            Self::SelectOrganization(selection)
            | Self::SelectProject { selection, .. }
            | Self::SelectTeam { selection, .. }
            | Self::SelectReadyColumn(selection) => Some(selection),
            _ => None,
        }
    }

    pub fn selection_mut(&mut self) -> Option<&mut Selection> {
        match self {
            Self::SelectOrganization(selection)
            | Self::SelectProject { selection, .. }
            | Self::SelectTeam { selection, .. }
            | Self::SelectReadyColumn(selection) => Some(selection),
            _ => None,
        }
    }

    pub fn input_mut(&mut self) -> Option<&mut String> {
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
    pub selected: usize,
}

impl Selection {
    pub fn new(items: Vec<String>) -> Self {
        Self { items, selected: 0 }
    }

    pub fn next(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn current(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
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
    fn empty_selection_has_no_current() {
        let mut selection = selection(&[]);

        selection.next();

        assert_eq!(selection.selected, 0);
        assert_eq!(selection.current(), None);
    }

    #[test]
    fn input_steps() {
        let mut pat = SetupStep::EnterPat {
            input: String::new(),
            error: None,
            cancelable: false,
        };
        let mut organization = SetupStep::EnterOrganization(String::new());

        assert!(pat.is_input());
        assert!(organization.is_input());
        pat.input_mut().unwrap().push('x');
        assert!(matches!(pat, SetupStep::EnterPat { input, .. } if input == "x"));
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
            SetupStep::SelectReadyColumn(selection(&["Ready"])),
        ];

        for step in &mut steps {
            assert!(step.selection().is_some(), "{}", step.title());
            assert!(step.selection_mut().is_some());
            assert!(!step.is_input());
        }
        assert!(SetupStep::Loading("…").selection().is_none());
    }
}
