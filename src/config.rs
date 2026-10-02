use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::theme::Theme;

const APP_NAME: &str = "azuredevopstui";
const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub organization: String,
    pub project: String,
    pub team: String,
    pub refresh_interval_secs: u64,
    pub browser_command: Option<String>,
    pub auth: AuthConfig,
    pub other_prs_filter: OtherPrsFilter,
    pub ready_column: String,
    pub colors: Theme,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            organization: "contoso".into(),
            project: "MyProject".into(),
            team: "My Team".into(),
            refresh_interval_secs: 120,
            browser_command: None,
            auth: AuthConfig::default(),
            other_prs_filter: OtherPrsFilter::default(),
            ready_column: "Ready".into(),
            colors: Theme::default(),
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir().context("could not resolve config directory")?;
        Ok(dir.join(APP_NAME).join(CONFIG_FILE))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&content)
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
