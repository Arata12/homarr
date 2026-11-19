use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadarrReleaseType {
    InCinemas,
    DigitalRelease,
    PhysicalRelease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CalendarMetadata {
    #[serde(rename_all = "camelCase")]
    Radarr { release_type: RadarrReleaseType },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarLink {
    pub name: String,
    pub is_dark: bool,
    pub href: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarImageBadge {
    pub content: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarImageAspectRatio {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarImage {
    pub src: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<CalendarImageBadge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<CalendarImageAspectRatio>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub title: String,
    pub sub_title: Option<String>,
    pub description: Option<String>,
    pub start_date: DateTime<Utc>,
    pub end_date: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_all_day: Option<bool>,
    pub image: Option<CalendarImage>,
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CalendarMetadata>,
    pub indicator_color: String,
    pub links: Vec<CalendarLink>,
}
