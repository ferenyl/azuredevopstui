use anyhow::Result;

use super::models::{Account, ConnectionData, CurrentUser, ListResponse, Named, Profile};
use super::{AdoClient, sorted_names};

const VSSPS_API_VERSION: &str = "7.1-preview";
const CONNECTION_DATA_API_VERSION: &str = "7.1-preview";

impl AdoClient {
    pub async fn organizations(&self) -> Result<Vec<String>> {
        let profile: Profile = self
            .get_versioned(
                &self.urls.vssps,
                &["_apis", "profile", "profiles", "me"],
                &[],
                VSSPS_API_VERSION,
            )
            .await?;
        let accounts: ListResponse<Account> = self
            .get_versioned(
                &self.urls.vssps,
                &["_apis", "accounts"],
                &[("memberId", &profile.id)],
                VSSPS_API_VERSION,
            )
            .await?;
        Ok(sorted_names(
            accounts
                .value
                .into_iter()
                .map(|account| account.account_name),
        ))
    }

    pub async fn current_user(&self, organization: &str) -> Result<CurrentUser> {
        let connection: ConnectionData = self
            .get_versioned(
                &self.urls.dev_azure,
                &[organization, "_apis", "connectionData"],
                &[],
                CONNECTION_DATA_API_VERSION,
            )
            .await?;
        Ok(connection.authenticated_user)
    }

    pub async fn projects(&self, organization: &str) -> Result<Vec<String>> {
        let projects: ListResponse<Named> = self
            .get(
                &self.urls.dev_azure,
                &[organization, "_apis", "projects"],
                &[("$top", "500")],
            )
            .await?;
        Ok(sorted_names(projects.value.into_iter().map(|p| p.name)))
    }

    pub async fn teams(&self, organization: &str, project: &str) -> Result<Vec<String>> {
        let teams: ListResponse<Named> = self
            .get(
                &self.urls.dev_azure,
                &[organization, "_apis", "projects", project, "teams"],
                &[("$top", "500")],
            )
            .await?;
        Ok(sorted_names(teams.value.into_iter().map(|t| t.name)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::api::is_unauthorized;
    use crate::auth::Auth;

    async fn client_with(status: u16, body: serde_json::Value) -> (MockServer, AdoClient) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/connectionData"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());
        (server, client)
    }

    #[tokio::test]
    async fn current_user_sends_pat_as_basic_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/connectionData"))
            .and(header("authorization", "Basic OnBhdA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "authenticatedUser": { "id": "user-id", "providerDisplayName": "Anna" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let user = client.current_user("contoso").await.unwrap();

        assert_eq!(user.id, "user-id");
    }

    #[tokio::test]
    async fn status_401_is_unauthorized() {
        let (_server, client) = client_with(401, json!({})).await;

        let err = client.current_user("contoso").await.unwrap_err();

        assert!(is_unauthorized(&err));
    }

    #[tokio::test]
    async fn status_203_is_unauthorized() {
        let (_server, client) = client_with(203, json!({})).await;

        let err = client.current_user("contoso").await.unwrap_err();

        assert!(is_unauthorized(&err));
    }

    #[tokio::test]
    async fn error_shows_azure_devops_message() {
        let (_server, client) = client_with(
            400,
            json!({ "message": "TF400813: The user is not authorized" }),
        )
        .await;

        let err = client.current_user("contoso").await.unwrap_err();

        assert!(!is_unauthorized(&err));
        assert_eq!(
            err.to_string(),
            "400 Bad Request: TF400813: The user is not authorized"
        );
    }

    #[tokio::test]
    async fn invalid_body_is_an_error() {
        let (_server, client) = client_with(200, json!({ "unexpected": true })).await;

        let err = client.current_user("contoso").await.unwrap_err();

        assert!(err.to_string().starts_with("invalid response"));
    }

    #[tokio::test]
    async fn projects_are_sorted_by_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/projects"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "name": "zeta" }, { "name": "Alpha" }
            ]})))
            .mount(&server)
            .await;
        let client = AdoClient::with_base_url(Auth::from_pat("pat"), &server.uri());

        let projects = client.projects("contoso").await.unwrap();

        assert_eq!(projects, ["Alpha", "zeta"]);
    }
}
