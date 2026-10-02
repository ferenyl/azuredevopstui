mod boards;
mod models;
mod projects;
mod pull_requests;
mod work_items;

pub use boards::ColumnTarget;
pub use models::{
    Board, CurrentUser, PullRequest, PullRequestDetails, SprintWorkItems, WorkItem, WorkItemDetails,
};

use std::fmt;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::auth::Auth;

const API_VERSION: &str = "7.1";
const DEV_AZURE: &str = "https://dev.azure.com";
const VSSPS: &str = "https://app.vssps.visualstudio.com";
const VSSPS_DEV_AZURE: &str = "https://vssps.dev.azure.com";

/// Service roots, replaceable so tests can point them at a mock server.
#[derive(Clone)]
struct BaseUrls {
    dev_azure: String,
    vssps: String,
    vssps_dev_azure: String,
}

impl Default for BaseUrls {
    fn default() -> Self {
        Self {
            dev_azure: DEV_AZURE.into(),
            vssps: VSSPS.into(),
            vssps_dev_azure: VSSPS_DEV_AZURE.into(),
        }
    }
}

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
    urls: BaseUrls,
}

impl AdoClient {
    pub fn new(auth: Auth) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth,
            urls: BaseUrls::default(),
        }
    }

    #[cfg(test)]
    fn with_base_url(auth: Auth, url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth,
            urls: BaseUrls {
                dev_azure: url.into(),
                vssps: url.into(),
                vssps_dev_azure: url.into(),
            },
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

    async fn patch<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        body: &Value,
    ) -> Result<T> {
        let url = build_url(base, segments, &[], API_VERSION)?;
        self.request(Method::PATCH, url, Some(body)).await
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
    ) -> Result<T> {
        let mut response = self.send(method.clone(), &url, body).await?;
        if is_unauthorized_status(response.status()) && !self.auth.uses_pat() {
            tracing::info!(path = url.path(), "token rejected, refreshing");
            self.auth.refresh().await?;
            response = self.send(method.clone(), &url, body).await?;
        }
        let status = response.status();
        tracing::debug!(%method, path = url.path(), %status, "request");
        if is_unauthorized_status(status) {
            return Err(Unauthorized).context(url.path().to_string());
        }
        if !status.is_success() {
            let body: Option<Value> = response.json().await.ok();
            match body.as_ref().and_then(|b| b["message"].as_str()) {
                Some(message) => bail!("{status}: {message}"),
                None => bail!("{status}: {}", url.path()),
            }
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
        let is_patch = method == Method::PATCH;
        let mut request = self.http.request(method, url.clone());
        if is_patch {
            request = request.header(CONTENT_TYPE, "application/json-patch+json");
        }
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

pub fn pull_request_url(organization: &str, project: &str, repository: &str, id: u32) -> Url {
    web_url(&[
        organization,
        project,
        "_git",
        repository,
        "pullrequest",
        &id.to_string(),
    ])
}

pub fn work_item_url(organization: &str, project: &str, id: u32) -> Url {
    web_url(&[organization, project, "_workitems", "edit", &id.to_string()])
}

fn web_url(segments: &[&str]) -> Url {
    let mut url = Url::parse(DEV_AZURE).expect("valid base url");
    url.path_segments_mut()
        .expect("base url can have path")
        .pop_if_empty()
        .extend(segments);
    url
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_encodes_segments_and_appends_api_version() {
        let url = build_url(
            "https://dev.azure.com",
            &["contoso", "My Team", "_apis", "wit"],
            &[("$top", "5")],
            "7.1",
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "https://dev.azure.com/contoso/My%20Team/_apis/wit?%24top=5&api-version=7.1"
        );
    }

    #[test]
    fn build_url_keeps_base_path() {
        let url = build_url(
            "https://vssps.dev.azure.com/contoso",
            &["_apis", "identities"],
            &[],
            "7.1",
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "https://vssps.dev.azure.com/contoso/_apis/identities?api-version=7.1"
        );
    }

    #[test]
    fn build_url_rejects_invalid_base() {
        assert!(build_url("not a url", &[], &[], "7.1").is_err());
    }

    #[test]
    fn pull_request_url_points_to_web_ui() {
        let url = pull_request_url("contoso", "My Project", "my-api", 42);

        assert_eq!(
            url.as_str(),
            "https://dev.azure.com/contoso/My%20Project/_git/my-api/pullrequest/42"
        );
    }

    #[test]
    fn work_item_url_points_to_web_ui() {
        let url = work_item_url("contoso", "MyProject", 7);

        assert_eq!(
            url.as_str(),
            "https://dev.azure.com/contoso/MyProject/_workitems/edit/7"
        );
    }

    #[test]
    fn unauthorized_statuses() {
        assert!(is_unauthorized_status(StatusCode::UNAUTHORIZED));
        assert!(is_unauthorized_status(
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        ));
        assert!(!is_unauthorized_status(StatusCode::OK));
        assert!(!is_unauthorized_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn is_unauthorized_finds_cause_in_context_chain() {
        let err = Err::<(), _>(Unauthorized)
            .context("/_apis/connectionData")
            .context("loading user")
            .unwrap_err();

        assert!(is_unauthorized(&err));
        assert!(!is_unauthorized(&anyhow!("other error")));
    }

    #[test]
    fn sorted_names_ignores_case() {
        let names = sorted_names(["beta", "Alpha", "gamma"].into_iter().map(String::from));

        assert_eq!(names, ["Alpha", "beta", "gamma"]);
    }
}
