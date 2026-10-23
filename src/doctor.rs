//! `azuredevopstui doctor`: checks the setup without opening the TUI.

use crate::api::{AdoClient, is_unauthorized};
use crate::auth::Auth;
use crate::config::Config;

#[derive(Debug, PartialEq, Eq)]
pub struct Check {
    pub ok: bool,
    pub text: String,
}

impl Check {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
        }
    }

    fn failed(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
        }
    }
}

/// Runs every check, stopping at the first that the next ones depend on.
pub async fn run() -> Vec<Check> {
    let mut checks = vec![Check::ok(format!(
        "azuredevopstui {}",
        env!("CARGO_PKG_VERSION")
    ))];
    let path = match Config::path() {
        Ok(path) => path,
        Err(err) => {
            checks.push(Check::failed(format!("{err:#}")));
            return checks;
        }
    };
    let config = match Config::load() {
        Ok(Some(config)) => config,
        Ok(None) => {
            checks.push(Check::failed(format!(
                "no config at {}, run azuredevopstui to set it up",
                path.display()
            )));
            return checks;
        }
        Err(err) => {
            checks.push(Check::failed(format!("{err:#}")));
            return checks;
        }
    };
    checks.push(Check::ok(format!("config {}", path.display())));
    let auth = match Auth::resolve(config.auth.clone()).await {
        Ok(Some(auth)) => auth,
        Ok(None) => {
            checks.push(Check::failed(
                "no PAT stored, run azuredevopstui to enter one",
            ));
            return checks;
        }
        Err(err) => {
            checks.push(Check::failed(format!("sign-in failed: {err:#}")));
            return checks;
        }
    };
    checks.push(Check::ok(if auth.uses_pat() {
        "credentials: PAT from the keyring"
    } else {
        "credentials: Azure CLI"
    }));
    checks.extend(check_project(&AdoClient::new(auth), &config).await);
    checks
}

/// Signs in to the organization and looks up the team in the project.
pub async fn check_project(client: &AdoClient, config: &Config) -> Vec<Check> {
    let organization = &config.organization;
    let user = match client.current_user(organization).await {
        Ok(user) => user,
        Err(err) if is_unauthorized(&err) => {
            return vec![Check::failed(format!(
                "not authorized for {organization}: run `az login` or enter a new PAT ({err:#})"
            ))];
        }
        Err(err) => return vec![Check::failed(format!("{organization}: {err:#}"))],
    };
    let mut checks = vec![Check::ok(format!(
        "signed in as {} to {organization}",
        user.provider_display_name
    ))];
    let (project, team) = (&config.project, &config.team);
    checks.push(match client.teams(organization, project).await {
        Ok(teams) if teams.contains(team) => Check::ok(format!("team {team} in {project}")),
        Ok(_) => Check::failed(format!("team {team} not found in {project}")),
        Err(err) => Check::failed(format!("project {project}: {err:#}")),
    });
    checks
}

/// One `✓`/`✗` line per check, and whether all passed.
pub fn render(checks: &[Check]) -> (String, bool) {
    let text = checks
        .iter()
        .map(|check| format!("{} {}\n", if check.ok { '✓' } else { '✗' }, check.text))
        .collect();
    (text, checks.iter().all(|check| check.ok))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::test_support::config;

    async fn server(user_status: u16, teams: &[&str]) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/connectionData"))
            .respond_with(ResponseTemplate::new(user_status).set_body_json(json!({
                "authenticatedUser": { "id": "user-id", "providerDisplayName": "Anna" }
            })))
            .mount(&server)
            .await;
        let teams: Vec<_> = teams.iter().map(|name| json!({ "name": name })).collect();
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/projects/MyProject/teams"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": teams })))
            .mount(&server)
            .await;
        server
    }

    async fn check(user_status: u16, teams: &[&str]) -> Vec<Check> {
        let server = server(user_status, teams).await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        check_project(&client, &config()).await
    }

    #[tokio::test]
    async fn signed_in_with_the_team() {
        let checks = check(200, &["Other", "My Team"]).await;

        assert_eq!(
            checks,
            [
                Check::ok("signed in as Anna to contoso"),
                Check::ok("team My Team in MyProject"),
            ]
        );
    }

    #[tokio::test]
    async fn unauthorized_suggests_signing_in_again() {
        let checks = check(401, &["My Team"]).await;

        assert_eq!(checks.len(), 1);
        assert!(!checks[0].ok);
        assert!(
            checks[0]
                .text
                .starts_with("not authorized for contoso: run `az login` or enter a new PAT"),
            "{}",
            checks[0].text
        );
    }

    #[tokio::test]
    async fn missing_team_fails() {
        let checks = check(200, &["Other"]).await;

        assert_eq!(
            checks.last(),
            Some(&Check::failed("team My Team not found in MyProject"))
        );
    }

    #[test]
    fn render_marks_each_check_and_fails_on_any_failure() {
        let passed = [Check::ok("a"), Check::ok("b")];
        let failed = [Check::ok("a"), Check::failed("b")];

        assert_eq!(render(&passed), ("✓ a\n✓ b\n".to_string(), true));
        assert_eq!(render(&failed), ("✓ a\n✗ b\n".to_string(), false));
    }
}
