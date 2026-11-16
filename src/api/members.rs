use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result, bail};
use futures::{StreamExt, stream};

use super::models::{Identity, ListResponse, Member, ProjectTeam, ProjectTeamMember};
use super::{MAX_CONCURRENT_REQUESTS, ProjectApi};

/// Most ids looked up in one identities request.
const IDS_PER_REQUEST: usize = 50;

/// Display names by identity id, to show `@<id>` mentions as `@Name`.
#[derive(Default)]
pub struct MentionNames(HashMap<String, String>);

impl MentionNames {
    /// `markdown` with every `@<id>` written as `@Name`, or escaped when the name is unknown, so
    /// it is not taken for an HTML tag and dropped.
    pub fn apply<'a>(&self, markdown: &'a str) -> Cow<'a, str> {
        if !markdown.contains("@<") {
            return Cow::Borrowed(markdown);
        }
        let mut named = String::with_capacity(markdown.len());
        let mut rest = markdown;
        while let Some(at) = rest.find("@<") {
            named.push_str(&rest[..at]);
            let after = &rest[at + 2..];
            let id = after
                .find('>')
                .map(|end| &after[..end])
                .filter(|id| is_id(id));
            match id {
                Some(id) => {
                    match self.0.get(&id.to_lowercase()) {
                        Some(name) => {
                            named.push('@');
                            named.push_str(name);
                        }
                        None => {
                            named.push_str("@\\<");
                            named.push_str(id);
                            named.push_str("\\>");
                        }
                    }
                    rest = &after[id.len() + 1..];
                }
                None => {
                    named.push_str("@<");
                    rest = after;
                }
            }
        }
        named.push_str(rest);
        Cow::Owned(named)
    }
}

/// The ids of the `@<id>` mentions in `text`, lowercased.
fn mentioned_ids(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split("@<").skip(1).filter_map(|part| {
        let id = &part[..part.find('>')?];
        is_id(id).then(|| id.to_lowercase())
    })
}

/// A person from the identities API; `None` for a group's pseudo account like `vstfs:` ones.
fn member_of_identity(identity: Identity) -> Option<Member> {
    let display_name = identity
        .custom_display_name
        .filter(|name| !name.is_empty())
        .unwrap_or(identity.provider_display_name);
    let unique_name = identity
        .properties
        .account
        .map_or_else(|| display_name.clone(), |account| account.value);
    (!display_name.is_empty() && !unique_name.starts_with("vstfs:")).then_some(Member {
        id: identity.id,
        display_name,
        unique_name,
    })
}

/// Identity ids are GUIDs.
fn is_id(text: &str) -> bool {
    !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_alphanumeric() || c == '-')
}

impl ProjectApi {
    /// The people on any team of the project, also through groups, by name, cached per session.
    /// Groups and inactive people are left out: they are the active users of the project, unlike
    /// the identity search, which also finds everyone in the directory.
    pub async fn project_members(&self) -> Result<Vec<Member>> {
        let members = self
            .project_members
            .get_or_try_init(|| self.load_project_members())
            .await?;
        Ok(members.clone())
    }

    async fn load_project_members(&self) -> Result<Vec<Member>> {
        if !self.mention_groups.is_empty() {
            return self.mention_group_members().await;
        }
        let teams: ListResponse<ProjectTeam> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    "_apis",
                    "projects",
                    &self.project,
                    "teams",
                ],
                &[("$top", "500")],
            )
            .await?;
        let teams: Vec<String> = teams.value.into_iter().map(|team| team.id).collect();
        match self.expanded_members(&teams).await {
            Ok(members) => Ok(members),
            Err(err) => {
                // Without Identity: Read, the people directly on a team are still found.
                tracing::warn!("failed to expand the teams, using their direct members: {err:#}");
                self.direct_team_members(&teams).await
            }
        }
    }

    /// The people in the configured `mention_groups`. A group that is not found is skipped,
    /// unless none is found.
    async fn mention_group_members(&self) -> Result<Vec<Member>> {
        let mut groups = Vec::new();
        let mut first_error = None;
        for name in self.mention_groups.iter() {
            match self.group_id(name).await {
                Ok(id) => groups.push(id),
                Err(err) => {
                    tracing::warn!("mention group {name}: {err:#}");
                    first_error.get_or_insert(err);
                }
            }
        }
        if groups.is_empty()
            && let Some(err) = first_error
        {
            return Err(err.context("none of the mention_groups was found"));
        }
        self.expanded_members(&groups).await
    }

    /// The id of the group named `name`, like `[Project]\\Developers` or an Entra ID group's name.
    async fn group_id(&self, name: &str) -> Result<String> {
        let groups: Vec<Identity> = self
            .find_identities(name)
            .await?
            .into_iter()
            .filter(|identity| identity.is_container)
            .collect();
        let named = |identity: &&Identity| {
            let full = &identity.provider_display_name;
            let short = full.rsplit('\\').next().unwrap_or(full);
            full.eq_ignore_ascii_case(name) || short.eq_ignore_ascii_case(name)
        };
        groups
            .iter()
            .find(named)
            .or(groups.first())
            .map(|group| group.id.clone())
            .with_context(|| format!("group not found: {name}"))
    }

    /// Everyone in the groups, such as teams or Entra ID groups, also through groups in them.
    /// Only active people, not groups.
    async fn expanded_members(&self, groups: &[String]) -> Result<Vec<Member>> {
        let groups: Vec<String> = groups.iter().map(|id| id.to_lowercase()).collect();
        let mut ids = BTreeSet::new();
        for chunk in groups.chunks(IDS_PER_REQUEST) {
            for group in self.identities_by_id(chunk, "ExpandedDown").await? {
                ids.extend(group.member_ids.iter().map(|id| id.to_lowercase()));
            }
        }
        for group in &groups {
            ids.remove(group);
        }
        if ids.is_empty() {
            bail!("the groups have no members");
        }
        let ids: Vec<String> = ids.into_iter().collect();
        let chunks: Vec<Vec<String>> = ids.chunks(IDS_PER_REQUEST).map(<[_]>::to_vec).collect();
        let chunks: Vec<Result<Vec<Identity>>> = stream::iter(chunks)
            .map(|chunk| async move { self.identities_by_id(&chunk, "None").await })
            .buffer_unordered(MAX_CONCURRENT_REQUESTS)
            .collect()
            .await;
        let mut members = Vec::new();
        for identities in chunks {
            members.extend(
                identities?
                    .into_iter()
                    .filter(|identity| !identity.is_container && identity.is_active)
                    .filter_map(member_of_identity),
            );
        }
        members.sort_by_key(|member| member.display_name.to_lowercase());
        Ok(members)
    }

    /// The people directly on the teams, without the members of groups.
    async fn direct_team_members(&self, teams: &[String]) -> Result<Vec<Member>> {
        let results: Vec<Result<Vec<ProjectTeamMember>>> = stream::iter(teams.to_vec())
            .map(|team| async move { self.members_of(&team).await })
            .buffer_unordered(MAX_CONCURRENT_REQUESTS)
            .collect()
            .await;
        let mut first_error = None;
        let mut seen = HashSet::new();
        let mut members: Vec<Member> = Vec::new();
        for result in results {
            match result {
                Ok(team) => members.extend(
                    team.into_iter()
                        .map(|member| member.identity)
                        .filter(|identity| !identity.is_container && !identity.inactive)
                        .filter(|identity| seen.insert(identity.id.to_lowercase()))
                        .map(|identity| Member {
                            unique_name: identity
                                .unique_name
                                .unwrap_or_else(|| identity.display_name.clone()),
                            id: identity.id,
                            display_name: identity.display_name,
                        })
                        .filter(|member| !member.unique_name.starts_with("vstfs:")),
                ),
                Err(err) => {
                    tracing::warn!("failed to load team members: {err:#}");
                    first_error.get_or_insert(err);
                }
            }
        }
        // A team the user cannot read is skipped, but with nothing read the list would be wrong.
        if members.is_empty()
            && let Some(err) = first_error
        {
            bail!(err);
        }
        members.sort_by_key(|member| member.display_name.to_lowercase());
        Ok(members)
    }

    /// Names of the people mentioned in `texts`: the project's members, then the others by id,
    /// looked up once per session. Asks for nothing when nobody is mentioned.
    pub(super) async fn mention_names<'a>(
        &self,
        texts: impl IntoIterator<Item = &'a str>,
    ) -> MentionNames {
        let ids: HashSet<String> = texts.into_iter().flat_map(mentioned_ids).collect();
        if ids.is_empty() {
            return MentionNames::default();
        }
        let members = self.project_members().await.unwrap_or_default();
        let mut names: HashMap<String, String> = members
            .into_iter()
            .map(|member| (member.id.to_lowercase(), member.display_name))
            .filter(|(id, _)| ids.contains(id))
            .collect();
        let missing: Vec<String> = {
            let known = self.identity_names.lock().expect("identity names");
            for id in &ids {
                if let Some(Some(name)) = known.get(id) {
                    names.insert(id.clone(), name.clone());
                }
            }
            ids.into_iter()
                .filter(|id| !names.contains_key(id) && !known.contains_key(id))
                .collect()
        };
        for chunk in missing.chunks(IDS_PER_REQUEST) {
            let found = match self.identities_by_id(chunk, "None").await {
                Ok(found) => found,
                Err(err) => {
                    // Shown as `@<id>`; the next load asks again.
                    tracing::warn!("failed to look up mentioned people: {err:#}");
                    continue;
                }
            };
            let mut known = self.identity_names.lock().expect("identity names");
            for id in chunk {
                let name = found
                    .iter()
                    .find(|identity| identity.id.eq_ignore_ascii_case(id))
                    .map(|identity| identity.provider_display_name.clone())
                    .filter(|name| !name.is_empty());
                if let Some(name) = &name {
                    names.insert(id.clone(), name.clone());
                }
                known.insert(id.clone(), name);
            }
        }
        MentionNames(names)
    }

    /// `membership` is a `queryMembership` value: `None`, or `ExpandedDown` for the members of
    /// groups, also through other groups.
    async fn identities_by_id(&self, ids: &[String], membership: &str) -> Result<Vec<Identity>> {
        let base = format!("{}/{}", self.client.urls.vssps_dev_azure, self.organization);
        let identities: ListResponse<Option<Identity>> = self
            .client
            .get(
                &base,
                &["_apis", "identities"],
                &[
                    ("identityIds", &ids.join(",")),
                    ("queryMembership", membership),
                ],
            )
            .await?;
        Ok(identities.value.into_iter().flatten().collect())
    }

    async fn members_of(&self, team: &str) -> Result<Vec<ProjectTeamMember>> {
        let members: ListResponse<ProjectTeamMember> = self
            .client
            .get(
                &self.client.urls.dev_azure,
                &[
                    &self.organization,
                    "_apis",
                    "projects",
                    &self.project,
                    "teams",
                    team,
                    "members",
                ],
                &[("$top", "500")],
            )
            .await?;
        Ok(members.value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn answer(server: &MockServer, url: &str, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(url))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    fn identity(id: &str, name: &str, extra: Value) -> Value {
        let mut identity = json!({
            "id": id,
            "displayName": name,
            "uniqueName": format!("{}@contoso.com", name.to_lowercase())
        });
        identity
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({ "identity": identity })
    }

    const TEAMS: &str = "/contoso/_apis/projects/MyProject/teams";

    #[test]
    fn mentions_become_names_and_unknown_ids_stay_visible() {
        let names = MentionNames(HashMap::from([("ann-id".into(), "Ann Berg".into())]));

        assert_eq!(
            names.apply("Hi @<ANN-ID>, @<bo-id> and @<not an id> @"),
            "Hi @Ann Berg, @\\<bo-id\\> and @<not an id> @"
        );
        assert!(matches!(names.apply("no mention"), Cow::Borrowed(_)));
        let ids: Vec<_> = mentioned_ids("@<A-1> x@<b> @<> @<c d>").collect();
        assert_eq!(ids, ["a-1", "b"]);
    }

    #[tokio::test]
    async fn the_group_named_like_the_name_is_preferred() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("filterValue", "devs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": "person", "providerDisplayName": "Devs" },
                { "id": "other", "providerDisplayName": "[p]\\Devs Alumni", "isContainer": true },
                { "id": "devs", "providerDisplayName": "[p]\\Devs", "isContainer": true }
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("filterValue", "Nope"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": "person", "providerDisplayName": "Nope" }
            ]})))
            .mount(&server)
            .await;
        let api = ProjectApi::with_base_url(&server.uri());

        assert_eq!(api.group_id("devs").await.unwrap(), "devs");
        assert!(api.group_id("Nope").await.is_err());
    }

    #[tokio::test]
    async fn members_of_groups_on_a_team_are_included() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        answer(&server, TEAMS, 200, json!({ "value": [{ "id": "T1" }] })).await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("identityIds", "t1"))
            .and(query_param("queryMembership", "ExpandedDown"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": "t1", "isContainer": true, "memberIds": ["G", "b", "a", "x", "t1"] }
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/contoso/_apis/identities"))
            .and(query_param("identityIds", "a,b,g,x"))
            .and(query_param("queryMembership", "None"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [
                { "id": "a", "providerDisplayName": "Ann", "customDisplayName": "Ann B",
                  "properties": { "Account": { "$value": "ann@contoso.com" } } },
                { "id": "b", "providerDisplayName": "Bo" },
                { "id": "g", "providerDisplayName": "Devs", "isContainer": true },
                { "id": "x", "providerDisplayName": "Gone", "isActive": false },
                null
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let api = ProjectApi::with_base_url(&server.uri());

        let members = api.project_members().await.unwrap();

        assert_eq!(
            members,
            [
                Member {
                    id: "a".into(),
                    display_name: "Ann B".into(),
                    unique_name: "ann@contoso.com".into()
                },
                Member {
                    id: "b".into(),
                    display_name: "Bo".into(),
                    unique_name: "Bo".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn members_of_all_teams_once_each_without_groups_or_inactive_people() {
        let server = MockServer::start().await;
        answer(
            &server,
            TEAMS,
            200,
            json!({ "value": [{ "id": "t1" }, { "id": "t2" }] }),
        )
        .await;
        answer(
            &server,
            &format!("{TEAMS}/t1/members"),
            200,
            json!({ "value": [
                identity("b", "Bo", json!({})),
                identity("g", "Readers", json!({ "isContainer": true })),
            ]}),
        )
        .await;
        answer(
            &server,
            &format!("{TEAMS}/t2/members"),
            200,
            json!({ "value": [
                identity("B", "Bo", json!({})),
                identity("a", "Ann", json!({})),
                identity("x", "Gone", json!({ "inactive": true })),
            ]}),
        )
        .await;
        let api = ProjectApi::with_base_url(&server.uri());

        let members = api.project_members().await.unwrap();

        let names: Vec<_> = members.iter().map(|m| m.display_name.as_str()).collect();
        assert_eq!(names, ["Ann", "Bo"]);
        assert_eq!(members[0].id, "a");
        assert_eq!(members[0].unique_name, "ann@contoso.com");
    }

    #[tokio::test]
    async fn a_team_that_fails_is_skipped_unless_all_do() {
        let server = MockServer::start().await;
        answer(
            &server,
            TEAMS,
            200,
            json!({ "value": [{ "id": "t1" }, { "id": "t2" }] }),
        )
        .await;
        answer(&server, &format!("{TEAMS}/t1/members"), 403, json!({})).await;
        answer(
            &server,
            &format!("{TEAMS}/t2/members"),
            200,
            json!({ "value": [identity("a", "Ann", json!({}))] }),
        )
        .await;
        let api = ProjectApi::with_base_url(&server.uri());

        let members = api.project_members().await.unwrap();

        assert_eq!(members.len(), 1);

        let server = MockServer::start().await;
        answer(&server, TEAMS, 200, json!({ "value": [{ "id": "t1" }] })).await;
        answer(&server, &format!("{TEAMS}/t1/members"), 403, json!({})).await;
        let api = ProjectApi::with_base_url(&server.uri());

        assert!(api.project_members().await.is_err());
    }
}
