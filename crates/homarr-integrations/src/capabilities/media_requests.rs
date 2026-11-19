use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaRequestMediaType {
    Movie,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaAvailability {
    Available,
    PartiallyAvailable,
    Processing,
    Requested,
    Pending,
    Unknown,
    Deleted,
    Blacklisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaRequestStatus {
    Pending,
    Approved,
    Declined,
    Failed,
    Completed,
}

/// Upstream (Overseerr / Jellyseerr) request status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UpstreamMediaRequestStatus {
    PendingApproval = 1,
    Approved = 2,
    Declined = 3,
    Failed = 4,
    Completed = 5,
}

/// Upstream (Overseerr / Jellyseerr) media availability codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UpstreamMediaAvailability {
    Unknown = 1,
    Pending = 2,
    Processing = 3,
    PartiallyAvailable = 4,
    Available = 5,
    JellyseerrBlacklistedOrOverseerrDeleted = 6,
    JellyseerrDeleted = 7,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerieSeason {
    pub id: i64,
    pub season_number: i64,
    pub name: String,
    pub episode_count: i64,
}

/// TS: `SeriesInformation | MovieInformation` (`seasons` only present for series).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInformation {
    pub id: i64,
    pub overview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seasons: Option<Vec<SerieSeason>>,
    pub poster_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_seasons: Option<Vec<i64>>,
}

/// TS: `Omit<RequestUser, "requestCount">`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequestUser {
    pub id: i64,
    pub display_name: String,
    pub avatar: String,
    pub link: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequest {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub media_type: MediaRequestMediaType,
    pub backdrop_image_url: String,
    pub poster_image_path: String,
    pub href: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_date: Option<DateTime<Utc>>,
    pub status: MediaRequestStatus,
    pub availability: MediaAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<MediaRequestUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestStats {
    pub total: u64,
    pub movie: u64,
    pub tv: u64,
    pub pending: u64,
    pub approved: u64,
    pub declined: u64,
    pub processing: u64,
    pub available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestUser {
    pub id: i64,
    pub display_name: String,
    pub avatar: String,
    pub request_count: u64,
    pub link: String,
}

/// TS: `MediaRequestStats` (`{ stats, users }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequestStatsWithUsers {
    pub stats: RequestStats,
    pub users: Vec<RequestUser>,
}
