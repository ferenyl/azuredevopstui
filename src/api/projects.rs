use anyhow::Result;
use serde::de::IgnoredAny;

use super::models::{Account, ListResponse, Named, Profile};
use super::{AdoClient, DEV_AZURE, VSSPS, sorted_names};

const VSSPS_API_VERSION: &str = "7.1-preview";
const CONNECTION_DATA_API_VERSION: &str = "7.1-preview";

impl AdoClient {
    pub async fn organizations(&self) -> Result<Vec<String>> {
        let profile: Profile = self
            .get_versioned(
                VSSPS,
                &["_apis", "profile", "profiles", "me"],
                &[],
                VSSPS_API_VERSION,
            )
            .await?;
        let accounts: ListResponse<Account> = self
            .get_versioned(
                VSSPS,
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

    pub async fn check_connection(&self, organization: &str) -> Result<()> {
        let _: IgnoredAny = self
            .get_versioned(
                DEV_AZURE,
                &[organization, "_apis", "connectionData"],
                &[],
                CONNECTION_DATA_API_VERSION,
            )
            .await?;
        Ok(())
    }

    pub async fn projects(&self, organization: &str) -> Result<Vec<String>> {
        let projects: ListResponse<Named> = self
            .get(
                DEV_AZURE,
                &[organization, "_apis", "projects"],
                &[("$top", "500")],
            )
            .await?;
        Ok(sorted_names(projects.value.into_iter().map(|p| p.name)))
    }

    pub async fn teams(&self, organization: &str, project: &str) -> Result<Vec<String>> {
        let teams: ListResponse<Named> = self
            .get(
                DEV_AZURE,
                &[organization, "_apis", "projects", project, "teams"],
                &[("$top", "500")],
            )
            .await?;
        Ok(sorted_names(teams.value.into_iter().map(|t| t.name)))
    }
}
