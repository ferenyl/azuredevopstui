//! End-to-end tests: the real `App` and `AdoClient` against a static mock Azure DevOps API.
//!
//! Every file in `fixtures/` is one canned endpoint. Requests no fixture or override answers fail
//! the test, so tests stay offline and deterministic. See `docs/e2e.md`.

mod changes;
mod completion;
mod flows;
mod lifecycle;
mod links;
mod notifications;
mod scroll;
mod theme;
mod threads;
mod vote;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde::Deserialize;
use serde_json::{Map, Value};
use wiremock::matchers::{any, body_string_contains, method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, Request, Respond, ResponseTemplate, Times};

use super::{App, Screen, SetupStep};
use crate::api::AdoClient;
use crate::auth::Auth;
use crate::config::Config;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/e2e/fixtures");
/// Lower wins: overrides beat fixtures with matchers on query or body, which beat plain ones.
const OVERRIDE_PRIORITY: u8 = 1;
const SPECIFIC_PRIORITY: u8 = 4;
const PLAIN_PRIORITY: u8 = 5;
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE: Duration = Duration::from_millis(100);
const SCREEN_WIDTH: u16 = 160;
const SCREEN_HEIGHT: u16 = 45;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    method: String,
    path: String,
    #[serde(default)]
    query: Map<String, Value>,
    body_contains: Option<String>,
    #[serde(default = "ok")]
    status: u16,
    #[serde(default)]
    body: Value,
}

fn ok() -> u16 {
    200
}

impl Fixture {
    fn mock(&self) -> Mock {
        let mut builder = Mock::given(method(self.method.as_str())).and(path(self.path.as_str()));
        for (key, value) in &self.query {
            builder = builder.and(query_param(
                key.as_str(),
                value.as_str().unwrap_or_default(),
            ));
        }
        if let Some(text) = &self.body_contains {
            builder = builder.and(body_string_contains(text.as_str()));
        }
        let specific = !self.query.is_empty() || self.body_contains.is_some();
        builder
            .respond_with(ResponseTemplate::new(self.status).set_body_json(&self.body))
            .with_priority(if specific {
                SPECIFIC_PRIORITY
            } else {
                PLAIN_PRIORITY
            })
    }
}

/// Mock Azure DevOps serving the committed fixtures.
pub struct MockAdo {
    server: MockServer,
}

impl MockAdo {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let mut files: Vec<_> = fs::read_dir(FIXTURES)
            .expect("fixtures directory")
            .map(|entry| entry.expect("fixture entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for file in files {
            fixture(&file).mock().mount(&server).await;
        }
        Mock::given(any())
            .respond_with(Unmocked)
            .with_priority(u8::MAX)
            .expect(0)
            .named("request without fixture")
            .mount(&server)
            .await;
        Self { server }
    }

    pub fn uri(&self) -> String {
        self.server.uri()
    }

    /// Answers matching requests with `status` and an Azure DevOps style error body, ahead of fixtures.
    pub async fn fail(&self, http_method: &str, request_path: &str, status: u16) {
        Mock::given(method(http_method))
            .and(path(request_path))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(serde_json::json!({ "message": "mock failure" })),
            )
            .with_priority(OVERRIDE_PRIORITY)
            .mount(&self.server)
            .await;
    }

    /// Answers the first matching request with `response`, then falls back to the fixtures.
    pub async fn fail_once(
        &self,
        http_method: &str,
        request_path: &str,
        response: ResponseTemplate,
    ) {
        Mock::given(method(http_method))
            .and(path(request_path))
            .respond_with(response)
            .up_to_n_times(1)
            .with_priority(OVERRIDE_PRIORITY)
            .expect(1)
            .mount(&self.server)
            .await;
    }

    /// How many requests with `http_method` have reached a path ending in `suffix`.
    pub async fn received(&self, http_method: &str, suffix: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|request| {
                request.method.as_str() == http_method && request.url.path().ends_with(suffix)
            })
            .count()
    }

    /// Every request received so far, as `METHOD /path`.
    pub async fn requests(&self) -> Vec<String> {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .collect()
    }

    /// Mounts `mock` ahead of fixtures; its `expect` is verified when the test ends.
    pub async fn expect(
        &self,
        mock: MockBuilder,
        response: ResponseTemplate,
        times: impl Into<Times>,
    ) {
        mock.respond_with(response)
            .with_priority(OVERRIDE_PRIORITY)
            .expect(times)
            .mount(&self.server)
            .await;
    }
}

impl MockAdo {
    /// Like [`MockAdo::expect`], but only the first `times` requests are answered with `response`;
    /// later ones fall through to the next mock or fixture.
    pub async fn expect_first(&self, mock: MockBuilder, response: ResponseTemplate, times: u64) {
        mock.respond_with(response)
            .up_to_n_times(times)
            .with_priority(OVERRIDE_PRIORITY)
            .expect(times)
            .mount(&self.server)
            .await;
    }
}

/// Names the request in the test output; the test then fails on the `expect(0)`.
struct Unmocked;

impl Respond for Unmocked {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        eprintln!("no fixture for {} {}", request.method, request.url);
        ResponseTemplate::new(404)
    }
}

fn fixture(file: &Path) -> Fixture {
    let content = fs::read_to_string(file).expect("readable fixture");
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("invalid fixture {}: {err}", file.display()))
}

pub fn config() -> Config {
    let mut config = Config::new("contoso".into(), "Fabrikam".into(), "Web".into());
    config.ready_column = Some("Ready".into());
    config.show_images = false;
    config
}

/// An `App` connected to a [`MockAdo`] with a PAT that never touches the keyring.
pub struct Harness {
    pub app: App,
    pub ado: MockAdo,
}

impl Harness {
    /// Mock with fixtures; call [`Harness::connect`] after mounting overrides.
    pub async fn new() -> Self {
        let ado = MockAdo::start().await;
        let mut app = App::new(Some(config()), None);
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("e2e-pat"),
            &ado.uri(),
        ));
        app.sync_api();
        Self { app, ado }
    }

    /// Starts from the signed-in point of `App::run` and waits until setup is done and loading settles.
    pub async fn connect(&mut self) {
        self.app.continue_setup();
        self.wait_for(|app| !matches!(app.screen, Screen::Setup(SetupStep::Loading(_))))
            .await;
        self.settle().await;
    }

    /// Handles messages until none arrives for a short while.
    pub async fn settle(&mut self) {
        while let Ok(Some(message)) = tokio::time::timeout(IDLE, self.app.rx.recv()).await {
            self.app.handle_message(message);
        }
    }

    /// Handles messages until `done` holds; panics with the screen after a timeout.
    pub async fn wait_for(&mut self, done: impl Fn(&App) -> bool) {
        let wait = async {
            while !done(&self.app) {
                let message = self.app.rx.recv().await.expect("message channel open");
                self.app.handle_message(message);
            }
        };
        if tokio::time::timeout(WAIT_TIMEOUT, wait).await.is_err() {
            panic!("timed out waiting; screen:\n{}", self.screen());
        }
    }

    /// Handles messages until the screen shows `text`.
    pub async fn wait_for_text(&mut self, text: &str) {
        let wait = async {
            while !self.screen().contains(text) {
                let message = self.app.rx.recv().await.expect("message channel open");
                self.app.handle_message(message);
            }
        };
        if tokio::time::timeout(WAIT_TIMEOUT, wait).await.is_err() {
            panic!("timed out waiting for {text:?}; screen:\n{}", self.screen());
        }
    }

    pub fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    pub fn is_main(&self) -> bool {
        matches!(self.app.screen, Screen::Main)
    }

    /// The rendered screen as text, one line per row.
    pub fn screen(&self) -> String {
        let mut terminal =
            Terminal::new(TestBackend::new(SCREEN_WIDTH, SCREEN_HEIGHT)).expect("test terminal");
        terminal
            .draw(|frame| crate::ui::render(frame, &self.app))
            .expect("render");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn config(&self) -> Arc<Config> {
        self.app.config.clone().expect("config")
    }
}
