use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
const EXPIRY_MARGIN_SECS: u64 = 60;
#[cfg(windows)]
const AZ: &str = "az.cmd";
#[cfg(not(windows))]
const AZ: &str = "az";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub access_token: String,
    #[serde(rename = "expires_on")]
    expires_on: u64,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        now.saturating_add(EXPIRY_MARGIN_SECS) >= self.expires_on
    }
}

/// Uses the login in `config_dir` (`AZURE_CONFIG_DIR`) when set, else the default one.
pub async fn fetch_token(config_dir: Option<&Path>) -> Result<Token> {
    let mut command = Command::new(AZ);
    if let Some(dir) = config_dir {
        command.env("AZURE_CONFIG_DIR", dir);
    }
    let output = command
        .args([
            "account",
            "get-access-token",
            "--resource",
            ADO_RESOURCE,
            "--output",
            "json",
        ])
        .output()
        .await
        .context("failed to run az, is Azure CLI installed and on PATH?")?;
    if !output.status.success() {
        let login = match config_dir {
            Some(dir) if cfg!(windows) => format!(
                "$env:AZURE_CONFIG_DIR=\"{}\"; az login --allow-no-subscriptions",
                dir.display()
            ),
            Some(dir) => format!(
                "AZURE_CONFIG_DIR={} az login --allow-no-subscriptions",
                dir.display()
            ),
            None => "az login --allow-no-subscriptions".into(),
        };
        bail!(
            "az login required, run `{login}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse az token")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(expires_on: u64) -> Token {
        Token {
            access_token: "token".into(),
            expires_on,
        }
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn parses_az_output() {
        let output = r#"{
            "accessToken": "eyJ0eXAi",
            "expiresOn": "2026-10-02 10:00:00.000000",
            "expires_on": 1790000000,
            "subscription": "00000000-0000-0000-0000-000000000000",
            "tenant": "00000000-0000-0000-0000-000000000000",
            "tokenType": "Bearer"
        }"#;

        let token: Token = serde_json::from_str(output).unwrap();

        assert_eq!(token.access_token, "eyJ0eXAi");
        assert_eq!(token.expires_on, 1_790_000_000);
    }

    #[test]
    fn past_token_is_expired() {
        assert!(token(now() - 10).is_expired());
    }

    #[test]
    fn token_inside_margin_is_expired() {
        assert!(token(now() + EXPIRY_MARGIN_SECS - 5).is_expired());
    }

    #[test]
    fn future_token_is_valid() {
        assert!(!token(now() + 3600).is_expired());
    }
}
