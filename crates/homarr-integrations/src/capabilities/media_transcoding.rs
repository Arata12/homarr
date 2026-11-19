use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TdarrQueueItemType {
    Transcode,
    HealthCheck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdarrQueueItem {
    pub id: String,
    pub health_check: String,
    pub transcode: String,
    pub file_path: String,
    pub file_size: f64,
    pub container: String,
    pub codec: String,
    pub resolution: String,
    #[serde(rename = "type")]
    pub item_type: TdarrQueueItemType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdarrQueue {
    pub array: Vec<TdarrQueueItem>,
    pub total_count: u64,
    pub start_index: u64,
    pub end_index: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdarrPieSegment {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdarrStatistics {
    pub library_name: String,
    pub total_file_count: u64,
    pub total_transcode_count: u64,
    pub total_health_check_count: u64,
    pub failed_transcode_count: u64,
    pub failed_health_check_count: u64,
    pub staged_transcode_count: u64,
    pub staged_health_check_count: u64,
    pub total_saved_space: f64,
    pub transcode_status: Vec<TdarrPieSegment>,
    pub health_check_status: Vec<TdarrPieSegment>,
    pub video_codecs: Vec<TdarrPieSegment>,
    pub video_containers: Vec<TdarrPieSegment>,
    pub video_resolutions: Vec<TdarrPieSegment>,
    pub audio_codecs: Vec<TdarrPieSegment>,
    pub audio_containers: Vec<TdarrPieSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdarrWorker {
    pub id: String,
    pub file_path: String,
    pub fps: f64,
    pub percentage: f64,
    /// Serialized as `ETA` (TS field name).
    #[serde(rename = "ETA")]
    pub eta: String,
    pub job_type: String,
    pub status: String,
    pub step: String,
    pub original_size: f64,
    pub estimated_size: Option<f64>,
    pub output_size: Option<f64>,
}
