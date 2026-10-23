use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use crate::theme::{ColorOverrides, ThemeName};

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
    /// Iteration path shown instead of the team's current sprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_sprint: Option<String>,
    /// Empty shows all types.
    #[serde(default)]
    pub work_item_types: Vec<String>,
    #[serde(default)]
    pub sort: SortConfig,
    /// Last merge strategy used to complete a pull request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<MergeStrategy>,
    /// Completion options preselected when completing a pull request.
    #[serde(default)]
    pub completion: CompletionDefaults,
    /// Detect terminal graphics and draw images. Off skips the terminal query entirely.
    #[serde(default = "default_true")]
    pub show_images: bool,
    /// Desktop notification (`notify-send`) for new inbox entries.
    #[serde(default)]
    pub notifications: bool,
    /// Per-kind switches and quiet hours; `notifications` stays the master switch.
    #[serde(default)]
    pub notify: NotifyConfig,
    /// Built-in palette: mocha, light or terminal (the terminal's own colors).
    #[serde(default)]
    pub theme: ThemeName,
    /// Overrides on top of `theme`.
    #[serde(default)]
    pub colors: ColorOverrides,
    /// Tags added by the user, most recent first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_tags: Vec<String>,
}

fn default_refresh_interval_secs() -> u64 {
    DEFAULT_REFRESH_INTERVAL_SECS
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyConfig {
    pub builds: bool,
    pub mentions: bool,
    /// Pull requests waiting on you and changes to your own.
    pub pull_requests: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            builds: true,
            mentions: true,
            pull_requests: true,
            quiet_hours: None,
        }
    }
}

/// Local `HH:MM` times between which nothing is notified. May wrap midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub start: String,
    pub end: String,
}

impl QuietHours {
    /// Whether `time` is in `[start, end)`. Invalid times never match; equal times cover no time.
    pub fn contains(&self, time: NaiveTime) -> bool {
        let parse = |text: &str| NaiveTime::parse_from_str(text.trim(), "%H:%M").ok();
        let (Some(start), Some(end)) = (parse(&self.start), parse(&self.end)) else {
            return false;
        };
        if start <= end {
            start <= time && time < end
        } else {
            time >= start || time < end
        }
    }
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
    pub show_approved: bool,
    pub show_drafts: bool,
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

/// Last completion toggles; bypassing policies and the merge message are never remembered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompletionDefaults {
    pub delete_source_branch: bool,
    pub transition_work_items: bool,
}

/// Serialized names match the Azure DevOps API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeStrategy {
    NoFastForward,
    Squash,
    Rebase,
    RebaseMerge,
}

impl MergeStrategy {
    pub const ALL: [Self; 4] = [
        Self::NoFastForward,
        Self::Squash,
        Self::Rebase,
        Self::RebaseMerge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NoFastForward => "Merge (no fast-forward)",
            Self::Squash => "Squash commit",
            Self::Rebase => "Rebase and fast-forward",
            Self::RebaseMerge => "Semi-linear merge",
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
            default_sprint: None,
            work_item_types: Vec::new(),
            sort: SortConfig::default(),
            merge_strategy: None,
            completion: CompletionDefaults::default(),
            show_images: true,
            notifications: false,
            notify: NotifyConfig::default(),
            theme: ThemeName::default(),
            colors: ColorOverrides::default(),
            recent_tags: Vec::new(),
        }
    }

    #[cfg(not(test))]
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir().context("could not resolve config directory")?;
        Ok(dir.join(APP_NAME).join(CONFIG_FILE))
    }

    /// Tests never touch the user's config.
    #[cfg(test)]
    pub fn path() -> Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("{APP_NAME}-test-{}", std::process::id()));
        Ok(dir.join(CONFIG_FILE))
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
        assert!(config.show_images);
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
        assert_eq!(loaded.colors, config.colors);
    }

    #[test]
    fn new_config_writes_no_colors() {
        let config = Config::new("o".into(), "p".into(), "t".into());

        let json = serde_json::to_value(&config).unwrap();

        assert_eq!(json["theme"], "mocha");
        assert_eq!(json["colors"], serde_json::json!({}));
    }

    #[test]
    fn theme_names_are_lowercase() {
        let json = r#"{"organization":"o","project":"p","team":"t","theme":"terminal"}"#;

        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.theme, ThemeName::Terminal);
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

    fn time(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    fn quiet(start: &str, end: &str) -> QuietHours {
        QuietHours {
            start: start.into(),
            end: end.into(),
        }
    }

    #[test]
    fn quiet_hours_within_a_day() {
        let hours = quiet("09:00", "17:30");

        assert!(!hours.contains(time(8, 59)));
        assert!(hours.contains(time(9, 0)));
        assert!(hours.contains(time(17, 29)));
        assert!(!hours.contains(time(17, 30)));
    }

    #[test]
    fn quiet_hours_wrap_midnight() {
        let hours = quiet("22:00", "07:00");

        assert!(hours.contains(time(22, 0)));
        assert!(hours.contains(time(23, 59)));
        assert!(hours.contains(time(0, 0)));
        assert!(hours.contains(time(6, 59)));
        assert!(!hours.contains(time(7, 0)));
        assert!(!hours.contains(time(12, 0)));
        assert!(!hours.contains(time(21, 59)));
    }

    #[test]
    fn invalid_or_empty_quiet_hours_never_match() {
        assert!(!quiet("late", "07:00").contains(time(23, 0)));
        assert!(!quiet("25:00", "07:00").contains(time(23, 0)));
        assert!(!quiet("08:00", "08:00").contains(time(8, 0)));
    }

    #[test]
    fn old_config_gets_notification_defaults() {
        let json = r#"{"organization":"o","project":"p","team":"t","notifications":true}"#;

        let config: Config = serde_json::from_str(json).unwrap();

        assert!(config.notifications);
        assert_eq!(config.notify, NotifyConfig::default());
        assert!(config.notify.builds && config.notify.mentions && config.notify.pull_requests);
        assert!(config.notify.quiet_hours.is_none());
    }

    #[test]
    fn notify_config_round_trips() {
        let json = r#"{"organization":"o","project":"p","team":"t","notify":{"mentions":false,"quiet_hours":{"start":"22:00","end":"07:00"}}}"#;

        let config: Config = serde_json::from_str(json).unwrap();
        let loaded: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();

        assert!(!loaded.notify.mentions);
        assert!(loaded.notify.builds && loaded.notify.pull_requests);
        assert_eq!(loaded.notify.quiet_hours, Some(quiet("22:00", "07:00")));
    }

    #[test]
    fn unset_quiet_hours_are_not_written() {
        let config = Config::new("o".into(), "p".into(), "t".into());

        let json = serde_json::to_value(&config).unwrap();

        assert!(json["notify"].get("quiet_hours").is_none());
    }
}
