use serde::{Deserialize, Serialize};

/// TS: `number | string`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IndexerId {
    Number(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Indexer {
    pub id: IndexerId,
    pub name: String,
    pub url: String,
    /// Whether the user enabled the indexer.
    pub enabled: bool,
    /// `Some(false)` when the indexer site reports an error; `None` when the provider does not expose health.
    pub status: Option<bool>,
}
