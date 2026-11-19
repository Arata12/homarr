use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaType {
    Movie,
    Tv,
    Music,
    Book,
    Game,
    Video,
    Article,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaReleaseImageUrls {
    pub poster: Option<String>,
    pub backdrop: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRelease {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: MediaType,
    pub title: String,
    /// Can also contain the season number for TV shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub release_date: DateTime<Utc>,
    /// The source supplied a calendar date without a time or timezone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date_is_date_only: Option<bool>,
    pub image_urls: MediaReleaseImageUrls,
    /// Studio, publisher or author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Price in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// Rating in any format (5/10, 90%, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    pub tags: Vec<String>,
    pub href: String,
    /// Video / music: duration in seconds. Book: number of pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
}
