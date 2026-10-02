use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListResponse<T> {
    pub value: Vec<T>,
}

#[derive(Deserialize)]
pub struct Profile {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub account_name: String,
}

#[derive(Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Deserialize)]
pub struct Backlog {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}
