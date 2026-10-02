mod azcli;
mod pat;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use reqwest::RequestBuilder;
use tokio::sync::Mutex;

use crate::config::{AuthConfig, AuthMethod};

enum Credential {
    AzCli {
        token: azcli::Token,
        config_dir: Option<PathBuf>,
    },
    Pat(String),
}

#[derive(Clone)]
pub struct Auth {
    credential: Arc<Mutex<Credential>>,
    uses_pat: bool,
}

impl Auth {
    fn new(credential: Credential) -> Self {
        Self {
            uses_pat: matches!(credential, Credential::Pat(_)),
            credential: Arc::new(Mutex::new(credential)),
        }
    }

    /// Returns `None` when no credential is available and a PAT has to be entered.
    pub async fn resolve(config: AuthConfig) -> Result<Option<Self>> {
        let config_dir = config.azure_config_dir();
        let az = |token| Credential::AzCli {
            token,
            config_dir: config_dir.clone(),
        };
        let credential = match config.method {
            AuthMethod::Azcli => Some(az(azcli::fetch_token(config_dir.as_deref()).await?)),
            AuthMethod::Pat => pat::load().await?.map(Credential::Pat),
            AuthMethod::Auto => match azcli::fetch_token(config_dir.as_deref()).await {
                Ok(token) => Some(az(token)),
                // A dedicated az login was configured, so show how to log in instead of asking for a PAT.
                Err(err) if config_dir.is_some() => return Err(err),
                Err(err) => {
                    tracing::warn!("az token unavailable, trying PAT: {err:#}");
                    pat::load().await?.map(Credential::Pat)
                }
            },
        };
        Ok(credential.map(Self::new))
    }

    #[cfg(test)]
    pub fn from_pat(pat: &str) -> Self {
        Self::new(Credential::Pat(pat.into()))
    }

    pub async fn with_new_pat(pat: String) -> Result<Self> {
        pat::save(pat.clone()).await?;
        Ok(Self::new(Credential::Pat(pat)))
    }

    pub fn uses_pat(&self) -> bool {
        self.uses_pat
    }

    pub async fn forget_pat() -> Result<()> {
        pat::delete().await
    }

    pub async fn refresh(&self) -> Result<()> {
        let mut credential = self.credential.lock().await;
        if let Credential::AzCli { token, config_dir } = &mut *credential {
            *token = azcli::fetch_token(config_dir.as_deref()).await?;
        }
        Ok(())
    }

    pub async fn authorize(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let mut credential = self.credential.lock().await;
        if let Credential::AzCli { token, config_dir } = &mut *credential
            && token.is_expired()
        {
            *token = azcli::fetch_token(config_dir.as_deref()).await?;
        }
        Ok(match &*credential {
            Credential::AzCli { token, .. } => request.bearer_auth(&token.access_token),
            Credential::Pat(pat) => request.basic_auth("", Some(pat)),
        })
    }
}
