use serde::{Deserialize, Serialize};

/// Result of `ISearchableIntegration.searchAsync` (`{ image?, name, link }` plus integration specific extras,
/// e.g. the Overseerr family fills `id`, `type`, `inLibrary` and `availability`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    pub link: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Upstream media type (`movie`, `tv`, `person`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub result_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_library: Option<bool>,
    /// A camelCase `MediaAvailability` value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
}
