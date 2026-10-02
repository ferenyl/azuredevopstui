use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::theme::Theme;

const APP_NAME: &str = "azuredevopstui";
const CONFIG_FILE: &str = "config.json";

const DEFAULT_REFRESH_INTERVAL_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub organization: String,
    pub project: String,
    pub team: String,
    #[serde(default = "default_refresh_interval_secs")]
    pub refresh_interval_secs: u64,
    #[serde(default)]
    pub browser_command: Option<String>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub other_prs_filter: OtherPrsFilter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_column: Option<String>,
    /// Empty shows all types.
    #[serde(default)]
    pub work_item_types: Vec<String>,
    #[serde(default)]
    pub sort: SortConfig,
    #[serde(default)]
    pub colors: Theme,
}

fn default_refresh_interval_secs() -> u64 {
    DEFAULT_REFRESH_INTERVAL_SECS
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub method: AuthMethod,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    #[default]
    Auto,
    Azcli,
    Pat,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OtherPrsFilter {
    pub reviewers: Vec<String>,
    pub creators: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SortConfig {
    pub pull_requests: PrSort,
    pub work_items: WorkItemSort,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrSort {
    #[default]
    Newest,
    Oldest,
    Title,
    Repository,
}

impl PrSort {
    pub const ALL: [Self; 4] = [Self::Newest, Self::Oldest, Self::Title, Self::Repository];

    pub fn label(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Oldest => "oldest",
            Self::Title => "title",
            Self::Repository => "repository",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemSort {
    #[default]
    Priority,
    Changed,
    Created,
    State,
    Id,
}

impl WorkItemSort {
    pub const ALL: [Self; 5] = [
        Self::Priority,
        Self::Changed,
        Self::Created,
        Self::State,
        Self::Id,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::Changed => "changed",
            Self::Created => "created",
            Self::State => "state",
            Self::Id => "id",
        }
    }
}

impl Config {
    pub fn new(organization: String, project: String, team: String) -> Self {
        Self {
            organization,
            project,
            team,
            refresh_interval_secs: DEFAULT_REFRESH_INTERVAL_SECS,
            browser_command: None,
            auth: AuthConfig::default(),
            other_prs_filter: OtherPrsFilter::default(),
            ready_column: None,
            work_item_types: Vec::new(),
            sort: SortConfig::default(),
            colors: Theme::default(),
        }
    }

    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir().context("could not resolve config directory")?;
        Ok(dir.join(APP_NAME).join(CONFIG_FILE))
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .map(Some)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
    }
}
