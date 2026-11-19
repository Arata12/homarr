use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaOrganizerItemType {
    Movie,
    Episode,
    Book,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingMediaItem {
    pub id: i64,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: MediaOrganizerItemType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMediaItem {
    pub id: i64,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: MediaOrganizerItemType,
    pub status: String,
    pub time_left: Option<String>,
    pub percent_complete: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingMediaPage {
    pub items: Vec<MissingMediaItem>,
    pub total_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMediaPage {
    pub items: Vec<QueuedMediaItem>,
    pub total_count: u64,
}
