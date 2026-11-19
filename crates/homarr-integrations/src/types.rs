use std::collections::HashMap;

use homarr_definitions::IntegrationKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct IntegrationContext {
    pub id: String,
    pub name: String,
    pub url: String,
    pub external_url: Option<String>,
    pub kind: IntegrationKind,
    pub secrets: HashMap<String, String>,
}

impl IntegrationContext {
    pub fn secret(&self, key: &str) -> Option<&str> {
        self.secrets.get(key).map(String::as_str)
    }

    pub fn require_secret(&self, key: &str) -> homarr_common::HomarrResult<&str> {
        self.secret(key).ok_or_else(|| {
            homarr_common::HomarrError::BadRequest(format!(
                "Integration credential '{key}' is missing for {}",
                self.name
            ))
        })
    }

    pub fn base_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpProxyRequest {
    pub integration_id: String,
    pub method: String,
    pub path: String,
    pub headers: Option<HashMap<String, String>>,
    pub query: Option<HashMap<String, String>>,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpProxyResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub sub_title: Option<String>,
    pub description: Option<String>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub image_url: Option<String>,
    pub link: Option<String>,
    pub indicator_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsHoleSummary {
    pub status: String,
    pub domains_being_blocked: u64,
    pub ads_blocked_today: u64,
    pub ads_percentage_today: f64,
    pub dns_queries_today: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientItem {
    pub id: String,
    pub name: String,
    pub state: String,
    pub size_bytes: u64,
    pub progress: f64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub eta_seconds: Option<u64>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadClientQueue {
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub paused: bool,
    pub items: Vec<DownloadClientItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerSession {
    pub id: String,
    pub username: String,
    pub item_name: String,
    pub series_name: Option<String>,
    pub client_name: String,
    pub is_paused: bool,
    pub progress_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequestItem {
    pub id: String,
    pub title: String,
    pub media_type: String,
    pub status: String,
    pub requested_by: String,
    pub created_at: String,
    pub poster_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRequestStats {
    pub total: u64,
    pub pending: u64,
    pub approved: u64,
    pub available: u64,
    pub declined: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthMonitoringDisk {
    pub name: String,
    pub mount_point: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthMonitoringInfo {
    pub system_name: String,
    pub version: String,
    pub cpu_percent: f64,
    pub cpu_temp_c: Option<f64>,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub uptime_seconds: u64,
    pub load_average: Option<[f64; 3]>,
    pub disks: Vec<HealthMonitoringDisk>,
    pub nodes: Vec<ClusterNodeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterNodeInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeEntity {
    pub entity_id: String,
    pub friendly_name: String,
    pub state: String,
    pub unit_of_measurement: Option<String>,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub priority: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTranscodingSummary {
    pub total_files: u64,
    pub queue_count: u64,
    pub space_saved_bytes: u64,
    pub active_workers: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaOrganizerItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsSummary {
    pub name: String,
    pub status: String,
    pub battery_charge_percent: f64,
    pub load_percent: f64,
    pub runtime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatMetric {
    pub label: String,
    pub value: String,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsResponse {
    pub integration_name: String,
    pub kind: String,
    pub metrics: Vec<StatMetric>,
}
