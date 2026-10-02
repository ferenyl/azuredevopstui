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
            Self::EnterOrganization(_) => "Enter organization",
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
