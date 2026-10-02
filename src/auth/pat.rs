use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "azuredevopstui";
const USER: &str = "pat";

pub async fn load() -> Result<Option<String>> {
    tokio::task::spawn_blocking(|| -> keyring::Result<Option<String>> {
        match Entry::new(SERVICE, USER)?.get_password() {
            Ok(pat) => Ok(Some(pat)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err),
        }
    })
    .await?
    .context("failed to read PAT from keyring")
}

pub async fn delete() -> Result<()> {
    tokio::task::spawn_blocking(|| -> keyring::Result<()> {
        match Entry::new(SERVICE, USER)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err),
        }
    })
    .await?
    .context("failed to delete PAT from keyring")
}

pub async fn save(pat: String) -> Result<()> {
    tokio::task::spawn_blocking(move || Entry::new(SERVICE, USER)?.set_password(&pat))
        .await?
        .context("failed to save PAT to keyring")
}
