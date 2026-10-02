mod boards;
mod models;
mod projects;

use std::fmt;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;

use crate::auth::Auth;

const API_VERSION: &str = "7.1";
const DEV_AZURE: &str = "https://dev.azure.com";
const VSSPS: &str = "https://app.vssps.visualstudio.com";

#[derive(Debug)]
pub struct Unauthorized;

impl fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not authorized")
    }
}

impl std::error::Error for Unauthorized {}

pub fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<Unauthorized>())
}

#[derive(Clone)]
pub struct AdoClient {
    http: reqwest::Client,
    auth: Auth,
}

impl AdoClient {
    pub fn new(auth: Auth) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth,
        }
    }

    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    async fn get<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T> {
        self.get_versioned(base, segments, query, API_VERSION).await
    }

    async fn get_versioned<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        query: &[(&str, &str)],
        api_version: &str,
    ) -> Result<T> {
        let mut url = Url::parse(base)?;
        url.path_segments_mut()
            .map_err(|_| anyhow!("invalid base url {base}"))?
            .pop_if_empty()
            .extend(segments);
        url.query_pairs_mut()
            .extend_pairs(query)
            .append_pair("api-version", api_version);

        let response = self
            .auth
            .authorize(self.http.get(url.clone()))
            .await?
            .send()
            .await
            .with_context(|| format!("request failed: {}", url.path()))?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::NON_AUTHORITATIVE_INFORMATION
        {
            return Err(Unauthorized).context(url.path().to_string());
        }
        if !status.is_success() {
            bail!("{status}: {}", url.path());
        }
        response
            .json()
            .await
            .with_context(|| format!("invalid response: {}", url.path()))
    }
}

fn sorted_names(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names.collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}
