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
    /// Separate Azure CLI login (`AZURE_CONFIG_DIR`) for the DevOps account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_config_dir: Option<String>,
}

impl AuthConfig {
    /// `azure_config_dir` with a leading `~` expanded to the home directory.
    pub fn azure_config_dir(&self) -> Option<PathBuf> {
        let dir = self.azure_config_dir.as_deref()?.trim();
        if dir.is_empty() {
            return None;
        }
        let rest = dir
            .strip_prefix('~')
            .filter(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'));
        match (rest, dirs::home_dir()) {
            (Some(rest), Some(home)) => Some(home.join(rest.trim_start_matches(['/', '\\']))),
            _ => Some(PathBuf::from(dir)),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(dir: Option<&str>) -> AuthConfig {
        AuthConfig {
            method: AuthMethod::Auto,
            azure_config_dir: dir.map(Into::into),
        }
    }

    #[test]
    fn minimal_config_gets_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"organization":"o","project":"p","team":"t"}"#).unwrap();

        assert_eq!(config.refresh_interval_secs, 120);
        assert_eq!(config.auth.method, AuthMethod::Auto);
        assert!(config.auth.azure_config_dir.is_none());
        assert!(config.browser_command.is_none());
        assert!(config.other_prs_filter.reviewers.is_empty());
        assert!(config.other_prs_filter.creators.is_empty());
        assert!(config.ready_column.is_none());
        assert!(config.work_item_types.is_empty());
        assert_eq!(config.sort.pull_requests, PrSort::Newest);
        assert_eq!(config.sort.work_items, WorkItemSort::Priority);
    }

    #[test]
    fn missing_required_field_is_an_error() {
        let result = serde_json::from_str::<Config>(r#"{"organization":"o","project":"p"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn sort_options_use_snake_case() {
        let sort = SortConfig {
            pull_requests: PrSort::Repository,
            work_items: WorkItemSort::Changed,
        };

        let json = serde_json::to_value(sort).unwrap();

        assert_eq!(
            json,
            serde_json::json!({ "pull_requests": "repository", "work_items": "changed" })
        );
    }

    #[test]
    fn unknown_sort_option_is_an_error() {
        let result = serde_json::from_str::<SortConfig>(r#"{"pull_requests":"random"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn sort_labels_match_serialized_names() {
        for sort in PrSort::ALL {
            assert_eq!(serde_json::to_value(sort).unwrap(), sort.label());
        }
        for sort in WorkItemSort::ALL {
            assert_eq!(serde_json::to_value(sort).unwrap(), sort.label());
        }
    }

    #[test]
    fn auth_method_uses_lowercase() {
        let auth: AuthConfig = serde_json::from_str(r#"{"method":"azcli"}"#).unwrap();

        assert_eq!(auth.method, AuthMethod::Azcli);
    }

    #[test]
    fn unset_optional_fields_are_not_written() {
        let config = Config::new("o".into(), "p".into(), "t".into());

        let json = serde_json::to_value(&config).unwrap();

        assert!(json.get("ready_column").is_none());
        assert!(json["auth"].get("azure_config_dir").is_none());
    }

    #[test]
    fn new_config_round_trips() {
        let mut config = Config::new("o".into(), "p".into(), "t".into());
        config.ready_column = Some("Ready".into());
        config.work_item_types = vec!["Bug".into()];

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.ready_column.as_deref(), Some("Ready"));
        assert_eq!(loaded.work_item_types, ["Bug"]);
        assert_eq!(loaded.colors.background, config.colors.background);
    }

    #[test]
    fn azure_config_dir_expands_tilde() {
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            auth(Some("~/.azure-devops")).azure_config_dir(),
            Some(home.join(".azure-devops"))
        );
        assert_eq!(auth(Some("~")).azure_config_dir(), Some(home));
    }

    #[test]
    fn azure_config_dir_keeps_other_paths() {
        assert_eq!(
            auth(Some("/opt/az")).azure_config_dir(),
            Some(PathBuf::from("/opt/az"))
        );
        assert_eq!(
            auth(Some("~other")).azure_config_dir(),
            Some(PathBuf::from("~other"))
        );
    }

    #[test]
    fn empty_azure_config_dir_is_none() {
        assert_eq!(auth(None).azure_config_dir(), None);
        assert_eq!(auth(Some("  ")).azure_config_dir(), None);
    }
}
