use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientSelection {
    pub sort: String,
    pub descending: bool,
    pub category_filter: Vec<String>,
    pub filter_is_whitelist: bool,
    pub show_completed_torrent: bool,
    pub active_torrent_threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadClientType {
    Torrent,
    Usenet,
    Miscellaneous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadClientItemState {
    Unknown,
    // usenet queue
    Downloading,
    Queued,
    Paused,
    // usenet history
    Completed,
    Failed,
    Processing,
    // torrent
    Leeching,
    Stalled,
    Seeding,
}

/// TS: `string | string[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DownloadClientCategory {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientItem {
    /// Unique identifier provided by the client.
    pub id: String,
    /// Position in queue.
    pub index: f64,
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: DownloadClientType,
    /// Size in bytes.
    pub size: f64,
    /// Total uploaded in bytes (torrents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<f64>,
    /// Total downloaded in bytes (torrents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<f64>,
    /// Bytes per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_speed: Option<f64>,
    /// Positive = ETA in ms (0 = infinite), negative = time since completion in ms.
    pub time: f64,
    /// Unix timestamp in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<f64>,
    pub state: DownloadClientItemState,
    /// Between 0 and 1.
    pub progress: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<DownloadClientCategory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientRates {
    pub down: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientStatus {
    pub paused: bool,
    pub rates: DownloadClientRates,
    pub types: Vec<DownloadClientType>,
}

/// TS: `Pick<DownloadClientItem, "sent" | "size" | "progress" | "category">`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientRatioItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<f64>,
    pub size: f64,
    pub progress: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<DownloadClientCategory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientJobsAndStatus {
    pub status: DownloadClientStatus,
    pub items: Vec<DownloadClientItem>,
    /// Original limited accounting population, independent of display selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio_items: Option<Vec<DownloadClientRatioItem>>,
}
