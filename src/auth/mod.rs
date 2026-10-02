mod azcli;
mod pat;

use std::sync::Arc;

use anyhow::Result;
use reqwest::RequestBuilder;
use tokio::sync::Mutex;

use crate::config::AuthMethod;

enum Credential {
    AzCli(azcli::Token),
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
    pub async fn resolve(method: AuthMethod) -> Result<Option<Self>> {
        let credential = match method {
            AuthMethod::Azcli => Some(Credential::AzCli(azcli::fetch_token().await?)),
            AuthMethod::Pat => pat::load().await?.map(Credential::Pat),
            AuthMethod::Auto => match azcli::fetch_token().await {
                Ok(token) => Some(Credential::AzCli(token)),
                Err(_) => pat::load().await?.map(Credential::Pat),
            },
        };
        Ok(credential.map(Self::new))
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

    pub async fn authorize(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let mut credential = self.credential.lock().await;
        if let Credential::AzCli(token) = &mut *credential
            && token.is_expired()
        {
            *token = azcli::fetch_token().await?;
        }
        Ok(match &*credential {
            Credential::AzCli(token) => request.bearer_auth(&token.access_token),
            Credential::Pat(pat) => request.basic_auth("", Some(pat)),
        })
    }
}
