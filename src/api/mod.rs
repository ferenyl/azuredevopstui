mod boards;
mod builds;
mod models;
mod projects;
mod pull_requests;
mod work_items;

pub use boards::ColumnTarget;
pub use models::{
    Board, Build, CurrentUser, Iteration, Link, LinkTarget, Mention, Partial, PullRequest,
    PullRequestDetails, ReviewSignals, SprintWorkItems, Thread, WorkItem, WorkItemDetails,
};
pub use pull_requests::{is_approved, is_reviewer_policy};

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::auth::Auth;
use crate::config::Config;

const API_VERSION: &str = "7.1";
const DEV_AZURE: &str = "https://dev.azure.com";
const VSSPS: &str = "https://app.vssps.visualstudio.com";
const VSSPS_DEV_AZURE: &str = "https://vssps.dev.azure.com";
const MAX_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_BASE: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("azuredevopstui/", env!("CARGO_PKG_VERSION"));

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
    backoff: Duration,
}

/// Builds the shared HTTP client. `request_timeout` bounds each whole request, including the body.
fn http_client(request_timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(request_timeout)
        .build()
        .expect("HTTP client with static settings")
}

impl AdoClient {
    pub fn new(auth: Auth) -> Self {
        Self {
            http: http_client(REQUEST_TIMEOUT),
            auth,
            urls: BaseUrls::default(),
            backoff: BACKOFF_BASE,
        }
    }

    #[cfg(test)]
    fn with_base_url(auth: Auth, url: &str) -> Self {
        Self {
            http: http_client(REQUEST_TIMEOUT),
            auth,
            urls: BaseUrls {
                dev_azure: url.into(),
                vssps: url.into(),
                vssps_dev_azure: url.into(),
            },
            backoff: Duration::from_millis(1),
        }
    }

    #[cfg(test)]
    fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
        self.http = http_client(request_timeout);
        self
    }

    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    /// Downloads an attachment such as an inline image. Credentials are only sent to Azure DevOps.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let url = Url::parse(url)?;
        if !self.is_trusted(&url) {
            bail!(
                "not an Azure DevOps url: {}",
                url.host_str().unwrap_or_default()
            );
        }
        let response = self.send(Method::GET, &url, None, true).await?;
        let status = response.status();
        if !status.is_success() {
            bail!("{status}: {}", url.path());
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
        {
            bail!("attachment too large: {}", url.path());
        }
        Ok(response.bytes().await?.to_vec())
    }

    fn is_trusted(&self, url: &Url) -> bool {
        let azure = url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host == "dev.azure.com" || host.ends_with(".visualstudio.com"));
        azure || Url::parse(&self.urls.dev_azure).is_ok_and(|base| base.origin() == url.origin())
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
        self.request(Method::GET, url, None, true).await
    }

    /// POST for read-only queries (WIQL, `workitemsbatch`); safe to retry. Mutations use `post_versioned`.
    async fn post_read<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        query: &[(&str, &str)],
        body: &Value,
    ) -> Result<T> {
        let url = build_url(base, segments, query, API_VERSION)?;
        self.request(Method::POST, url, Some(body), true).await
    }

    async fn post_versioned<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        query: &[(&str, &str)],
        body: &Value,
        api_version: &str,
    ) -> Result<T> {
        let url = build_url(base, segments, query, api_version)?;
        self.request(Method::POST, url, Some(body), false).await
    }

    async fn patch<T: DeserializeOwned>(
        &self,
        base: &str,
        segments: &[&str],
        body: &Value,
    ) -> Result<T> {
        let url = build_url(base, segments, &[], API_VERSION)?;
        self.request(Method::PATCH, url, Some(body), false).await
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
        retry: bool,
    ) -> Result<T> {
        let mut response = self.send(method.clone(), &url, body, retry).await?;
        if is_unauthorized_status(response.status()) && !self.auth.uses_pat() {
            tracing::info!(path = url.path(), "token rejected, refreshing");
            self.auth.refresh().await?;
            response = self.send(method.clone(), &url, body, retry).await?;
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

    /// Sends the request; with `retry`, transient failures are retried up to `MAX_ATTEMPTS` times in all.
    async fn send(
        &self,
        method: Method,
        url: &Url,
        body: Option<&Value>,
        retry: bool,
    ) -> Result<reqwest::Response> {
        let attempts = if retry { MAX_ATTEMPTS } else { 1 };
        let mut attempt = 1;
        loop {
            let result = self.send_once(method.clone(), url, body).await;
            let delay = match &result {
                Ok(response) if is_retryable_status(response.status()) => {
                    retry_after(response).map(|delay| delay.min(MAX_RETRY_DELAY))
                }
                Err(err) if is_transient_error(err) => None,
                _ => return result,
            };
            if attempt >= attempts {
                return result;
            }
            let delay = delay.unwrap_or_else(|| backoff_delay(self.backoff, attempt));
            tracing::info!(
                path = url.path(),
                attempt,
                ?delay,
                "transient failure, retrying"
            );
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }

    async fn send_once(
        &self,
        method: Method,
        url: &Url,
        body: Option<&Value>,
    ) -> Result<reqwest::Response> {
        let is_json_patch = method == Method::PATCH && body.is_some_and(Value::is_array);
        let mut request = self.http.request(method, url.clone());
        if is_json_patch {
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

/// An `AdoClient` bound to the configured organization, project and team.
#[derive(Clone)]
pub struct ProjectApi {
    client: AdoClient,
    organization: Arc<str>,
    project: Arc<str>,
    team: Arc<str>,
}

impl ProjectApi {
    pub fn new(client: AdoClient, config: &Config) -> Self {
        Self::for_team(client, &config.organization, &config.project, &config.team)
    }

    #[cfg(test)]
    fn with_base_url(url: &str) -> Self {
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), url);
        Self::for_team(client, "contoso", "MyProject", "Team")
    }

    #[cfg(test)]
    fn with_team(mut self, team: &str) -> Self {
        self.team = team.into();
        self
    }

    pub fn for_team(client: AdoClient, organization: &str, project: &str, team: &str) -> Self {
        Self {
            client,
            organization: organization.into(),
            project: project.into(),
            team: team.into(),
        }
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

pub fn build_results_url(organization: &str, project: &str, id: u32) -> Url {
    let mut url = web_url(&[organization, project, "_build", "results"]);
    url.query_pairs_mut()
        .append_pair("buildId", &id.to_string());
    url
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

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 502 | 503 | 504)
}

fn is_transient_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|err| err.is_connect() || err.is_timeout())
    })
}

/// Seconds from a `Retry-After` header; the HTTP-date form is ignored.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let seconds = response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds))
}

/// `base`, `2 * base`, ... plus up to 25% jitter.
fn backoff_delay(base: Duration, attempt: u32) -> Duration {
    let delay = base * 2u32.pow(attempt - 1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    delay + delay / 4 * (nanos % 1000) / 1000
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

    #[tokio::test]
    async fn download_sends_credentials_to_azure_devops() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/wit/attachments/abc"))
            .and(header("authorization", "Basic OnBhdA=="))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let bytes = client
            .download(&format!(
                "{}/contoso/_apis/wit/attachments/abc",
                server.uri()
            ))
            .await
            .unwrap();

        assert_eq!(bytes, [1, 2, 3]);
    }

    #[tokio::test]
    async fn requests_send_user_agent() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_apis/test"))
            .and(header(
                "user-agent",
                concat!("azuredevopstui/", env!("CARGO_PKG_VERSION")),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        client
            .get::<Value>(&server.uri(), &["_apis", "test"], &[])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn stalled_request_fails_with_timeout() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_apis/test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(5))
                    .set_body_json(serde_json::json!({})),
            )
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri())
            .with_request_timeout(Duration::from_millis(100));

        let err = client
            .get::<Value>(&server.uri(), &["_apis", "test"], &[])
            .await
            .unwrap_err();

        assert!(err.chain().any(|cause| {
            cause
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_timeout)
        }));
    }

    async fn serve_in_turn(
        server: &wiremock::MockServer,
        verb: &str,
        responses: Vec<wiremock::ResponseTemplate>,
    ) {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};
        for response in responses {
            Mock::given(method(verb))
                .and(path("/_apis/test"))
                .respond_with(response)
                .up_to_n_times(1)
                .expect(1)
                .mount(server)
                .await;
        }
    }

    #[tokio::test]
    async fn get_retries_throttling_with_retry_after() {
        use wiremock::{MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        serve_in_turn(
            &server,
            "GET",
            vec![
                ResponseTemplate::new(429).insert_header("retry-after", "0"),
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
            ],
        )
        .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let value: Value = client
            .get(&server.uri(), &["_apis", "test"], &[])
            .await
            .unwrap();

        assert_eq!(value["ok"], true);
    }

    #[tokio::test]
    async fn get_gives_up_after_three_attempts() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_apis/test"))
            .respond_with(ResponseTemplate::new(503))
            .expect(3)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let err = client
            .get::<Value>(&server.uri(), &["_apis", "test"], &[])
            .await
            .unwrap_err();

        assert!(err.to_string().starts_with("503"));
    }

    #[tokio::test]
    async fn get_does_not_retry_unauthorized_or_client_errors() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        for status in [401, 404] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/_apis/test"))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;
            let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

            let result = client
                .get::<Value>(&server.uri(), &["_apis", "test"], &[])
                .await;

            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn mutations_are_not_retried() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        for verb in ["POST", "PATCH"] {
            let server = MockServer::start().await;
            Mock::given(method(verb))
                .and(path("/_apis/test"))
                .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
                .expect(1)
                .mount(&server)
                .await;
            let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
            let body = serde_json::json!({});

            let result = if verb == "POST" {
                client
                    .post_versioned::<Value>(&server.uri(), &["_apis", "test"], &[], &body, "7.1")
                    .await
            } else {
                client
                    .patch::<Value>(&server.uri(), &["_apis", "test"], &body)
                    .await
            };

            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn read_only_post_is_retried() {
        use wiremock::{MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        serve_in_turn(
            &server,
            "POST",
            vec![
                ResponseTemplate::new(502),
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
            ],
        )
        .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let value: Value = client
            .post_read(
                &server.uri(),
                &["_apis", "test"],
                &[],
                &serde_json::json!({}),
            )
            .await
            .unwrap();

        assert_eq!(value["ok"], true);
    }

    #[tokio::test]
    async fn timeouts_are_retried() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_apis/test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(5))
                    .set_body_json(serde_json::json!({})),
            )
            .expect(3)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri())
            .with_request_timeout(Duration::from_millis(50));

        let result = client
            .get::<Value>(&server.uri(), &["_apis", "test"], &[])
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn backoff_doubles_with_bounded_jitter() {
        let base = Duration::from_secs(1);

        let first = backoff_delay(base, 1);
        let second = backoff_delay(base, 2);

        assert!(first >= base && first <= base * 5 / 4);
        assert!(second >= base * 2 && second <= base * 5 / 2);
    }

    #[test]
    fn retryable_statuses() {
        for status in [429, 502, 503, 504] {
            assert!(is_retryable_status(StatusCode::from_u16(status).unwrap()));
        }
        for status in [200, 400, 401, 404, 500] {
            assert!(!is_retryable_status(StatusCode::from_u16(status).unwrap()));
        }
    }

    #[tokio::test]
    async fn download_refuses_other_hosts() {
        let client = AdoClient::new(Auth::from_pat("pat"));

        let err = client
            .download("https://evil.example.com/image.png")
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "not an Azure DevOps url: evil.example.com");
    }

    #[test]
    fn trusted_hosts_are_azure_devops_over_https() {
        let client = AdoClient::new(Auth::from_pat("pat"));
        let trusted = |url: &str| client.is_trusted(&Url::parse(url).unwrap());

        assert!(trusted("https://dev.azure.com/o/_apis/wit/attachments/1"));
        assert!(trusted("https://contoso.visualstudio.com/_apis/x"));
        assert!(!trusted("http://dev.azure.com/o/x"));
        assert!(!trusted("https://dev.azure.com.evil.com/x"));
        assert!(!trusted("https://example.com/x"));
    }
}
