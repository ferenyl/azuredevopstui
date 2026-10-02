mod boards;
mod models;
mod projects;
mod pull_requests;
mod work_items;

pub use models::{
    CurrentUser, PullRequest, PullRequestDetails, SprintWorkItems, WorkItem, WorkItemDetails,
};

use std::fmt;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;

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
        let url = build_url(base, segments, query, api_version)?;
        self.request(Method::GET, url, None).await
    }

    async fn post<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        query: &[(&str, &str)],
        body: &Value,
    ) -> Result<T> {
        let url = build_url(base, segments, query, API_VERSION)?;
        self.request(Method::POST, url, Some(body)).await
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
    ) -> Result<T> {
        let mut response = self.send(method.clone(), &url, body).await?;
        if is_unauthorized_status(response.status()) && !self.auth.uses_pat() {
            self.auth.refresh().await?;
            response = self.send(method, &url, body).await?;
        }
        let status = response.status();
        if is_unauthorized_status(status) {
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

    async fn send(
        &self,
        method: Method,
        url: &Url,
        body: Option<&Value>,
    ) -> Result<reqwest::Response> {
        let mut request = self.http.request(method, url.clone());
        if let Some(body) = body {
            request = request.json(body);
        }
        self.auth
            .authorize(request)
            .await?
            .send()
            .await
            .with_context(|| format!("request failed: {}", url.path()))
    }
}

fn build_url(
    base: &str,
    segments: &[&str],
    query: &[(&str, &str)],
    api_version: &str,
) -> Result<Url> {
    let mut url = Url::parse(base)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid base url {base}"))?
        .pop_if_empty()
        .extend(segments);
    url.query_pairs_mut()
        .extend_pairs(query)
        .append_pair("api-version", api_version);
    Ok(url)
}

fn is_unauthorized_status(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::NON_AUTHORITATIVE_INFORMATION
}

fn sorted_names(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names.collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}
