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

pub async fn fetch_token() -> Result<Token> {
    let output = Command::new(AZ)
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
        bail!(
            "az login required: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse az token")
}
