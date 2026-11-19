//! Tdarr (media transcoding). Port of `media-transcoding/tdarr-integration.ts`.

use std::collections::BTreeMap;

use homarr_common::HomarrResult;
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::json;

use super::util::{send, send_json, url};
use crate::capabilities::media_transcoding::{
    TdarrPieSegment, TdarrQueue, TdarrQueueItem, TdarrQueueItemType, TdarrStatistics, TdarrWorker,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const GB: f64 = 1_000_000_000.0;
const MB: f64 = 1_000_000.0;

fn request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    post: bool,
    path: &str,
    json_headers: bool,
) -> HomarrResult<RequestBuilder> {
    let target = url(ctx, path, &[])?;
    let mut builder = if post {
        rt.http.standard().post(target)
    } else {
        rt.http.standard().get(target)
    };
    if json_headers {
        builder = builder.header("Content-Type", "application/json");
    }
    if let Some(key) = ctx.secret("apiKey") {
        builder = builder.header("X-Api-Key", key);
    }
    Ok(builder)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let builder = request(rt, ctx, true, "/api/v2/is-server-alive", false)?
        .header("accept", "application/json");
    send(ctx, builder).await?;
    Ok(())
}

// ---- statistics ------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Segment {
    name: String,
    value: f64,
}

type Segments = Option<Vec<Segment>>;

#[derive(Debug, Deserialize)]
struct StatusPies {
    #[serde(default)]
    transcode: Segments,
    #[serde(default)]
    healthcheck: Segments,
}

#[derive(Debug, Deserialize)]
struct VideoPies {
    #[serde(default)]
    codecs: Segments,
    #[serde(default)]
    containers: Segments,
    #[serde(default)]
    resolutions: Segments,
}

#[derive(Debug, Deserialize)]
struct AudioPies {
    #[serde(default)]
    codecs: Segments,
    #[serde(default)]
    containers: Segments,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PieStats {
    total_files: f64,
    total_transcode_count: f64,
    size_diff: f64,
    total_health_check_count: f64,
    status: StatusPies,
    video: VideoPies,
    audio: AudioPies,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatisticsResponse {
    pie_stats: PieStats,
}

fn count(value: f64) -> u64 {
    value.max(0.0) as u64
}

fn into_segments(segments: Segments) -> Vec<TdarrPieSegment> {
    segments
        .unwrap_or_default()
        .into_iter()
        .map(|segment| TdarrPieSegment {
            name: segment.name,
            value: segment.value,
        })
        .collect()
}

/// The Tdarr API only returns a category if there is at least one item in it.
fn find_value(segments: &[TdarrPieSegment], name: &str) -> u64 {
    segments
        .iter()
        .find(|segment| segment.name == name)
        .map_or(0, |segment| count(segment.value))
}

fn map_statistics(data: StatisticsResponse) -> TdarrStatistics {
    let stats = data.pie_stats;
    let transcode = into_segments(stats.status.transcode);
    let health_check = into_segments(stats.status.healthcheck);
    TdarrStatistics {
        library_name: "All".to_string(),
        total_file_count: count(stats.total_files),
        total_transcode_count: count(stats.total_transcode_count),
        total_health_check_count: count(stats.total_health_check_count),
        failed_transcode_count: find_value(&transcode, "Transcode error"),
        failed_health_check_count: find_value(&health_check, "Error"),
        staged_transcode_count: find_value(&transcode, "Transcode success"),
        staged_health_check_count: find_value(&health_check, "Queued"),
        // sizeDiff is in GB, convert to bytes
        total_saved_space: stats.size_diff * GB,
        transcode_status: transcode,
        health_check_status: health_check,
        video_codecs: into_segments(stats.video.codecs),
        video_containers: into_segments(stats.video.containers),
        video_resolutions: into_segments(stats.video.resolutions),
        audio_codecs: into_segments(stats.audio.codecs),
        audio_containers: into_segments(stats.audio.containers),
    }
}

pub async fn statistics(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<TdarrStatistics> {
    let builder = request(rt, ctx, true, "/api/v2/stats/get-pies", true)?
        .header("accept", "application/json")
        // empty string to get all libraries
        .json(&json!({ "data": { "libraryId": "" } }));
    let data: StatisticsResponse = send_json(ctx, "the statistics", builder).await?;
    Ok(map_statistics(data))
}

// ---- workers ---------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Job {
    #[serde(rename = "type")]
    job_type: String,
}

#[derive(Debug, Deserialize)]
struct PluginDetails {
    #[serde(default)]
    number: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkerResponse {
    #[serde(rename = "_id")]
    id: String,
    file: String,
    fps: f64,
    percentage: f64,
    #[serde(rename = "ETA")]
    eta: String,
    job: Job,
    status: String,
    #[serde(default, rename = "lastPluginDetails")]
    last_plugin_details: Option<PluginDetails>,
    #[serde(rename = "originalfileSizeInGbytes")]
    original_file_size_in_gbytes: f64,
    #[serde(default, rename = "estSize")]
    est_size: Option<f64>,
    #[serde(default, rename = "outputFileSizeInGbytes")]
    output_file_size_in_gbytes: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct NodeResponse {
    workers: BTreeMap<String, WorkerResponse>,
}

fn map_workers(nodes: BTreeMap<String, NodeResponse>) -> Vec<TdarrWorker> {
    nodes
        .into_values()
        .flat_map(|node| node.workers.into_values())
        .map(|worker| TdarrWorker {
            id: worker.id,
            file_path: worker.file,
            fps: worker.fps,
            percentage: worker.percentage,
            eta: worker.eta,
            job_type: worker.job.job_type,
            status: worker.status,
            step: worker
                .last_plugin_details
                .and_then(|details| details.number)
                .unwrap_or_default(),
            original_size: worker.original_file_size_in_gbytes * GB,
            // A zero value counts as missing (JavaScript truthiness).
            estimated_size: worker
                .est_size
                .filter(|size| *size != 0.0)
                .map(|size| size * GB),
            output_size: worker
                .output_file_size_in_gbytes
                .filter(|size| *size != 0.0)
                .map(|size| size * GB),
        })
        .collect()
}

pub async fn workers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<TdarrWorker>> {
    let builder = request(rt, ctx, false, "/api/v2/get-nodes", true)?;
    let nodes: BTreeMap<String, NodeResponse> = send_json(ctx, "the nodes", builder).await?;
    Ok(map_workers(nodes))
}

// ---- queue -----------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StatusRow {
    #[serde(rename = "_id")]
    id: String,
    #[serde(rename = "HealthCheck")]
    health_check: String,
    #[serde(rename = "TranscodeDecisionMaker")]
    transcode_decision_maker: String,
    file: String,
    file_size: f64,
    container: String,
    video_codec_name: String,
    video_resolution: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusTable {
    array: Vec<StatusRow>,
    total_count: u64,
}

fn map_rows(table: StatusTable, item_type: TdarrQueueItemType) -> (Vec<TdarrQueueItem>, u64) {
    let items = table
        .array
        .into_iter()
        .map(|row| TdarrQueueItem {
            id: row.id,
            health_check: row.health_check,
            transcode: row.transcode_decision_maker,
            file_path: row.file,
            // file_size is in MB, convert to bytes (it returns as float)
            file_size: (row.file_size * MB).floor(),
            container: row.container,
            codec: row.video_codec_name,
            resolution: row.video_resolution,
            item_type,
        })
        .collect();
    (items, table.total_count)
}

async fn status_table(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    start: usize,
    page_size: usize,
    table: &str,
) -> HomarrResult<StatusTable> {
    let builder = request(rt, ctx, true, "/api/v2/client/status-tables", true)?.json(&json!({
        "data": {
            "start": start,
            "pageSize": page_size,
            "filters": [],
            "sorts": [],
            "opts": { "table": table },
        }
    }));
    send_json(ctx, "the queue", builder).await
}

pub async fn queue(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    first_item_index: usize,
    page_size: usize,
) -> HomarrResult<TdarrQueue> {
    let transcodes = status_table(rt, ctx, first_item_index, page_size, "table1").await?;
    let (transcode_items, transcode_total) = map_rows(transcodes, TdarrQueueItemType::Transcode);
    // Health checks continue after the transcodes in the combined list.
    let health_start = first_item_index.saturating_sub(transcode_total as usize);
    let health = status_table(rt, ctx, health_start, page_size, "table4").await?;
    let (health_items, health_total) = map_rows(health, TdarrQueueItemType::HealthCheck);

    let array: Vec<TdarrQueueItem> = transcode_items
        .into_iter()
        .chain(health_items)
        .take(page_size)
        .collect();
    Ok(TdarrQueue {
        total_count: transcode_total + health_total,
        start_index: first_item_index as u64,
        end_index: (first_item_index + array.len()).saturating_sub(1) as u64,
        array,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_use_named_segments_and_convert_gb_to_bytes() {
        let data: StatisticsResponse = serde_json::from_value(json!({
            "pieStats": {
                "totalFiles": 120, "totalTranscodeCount": 80, "sizeDiff": 1.5,
                "totalHealthCheckCount": 100,
                "status": {
                    "transcode": [{ "name": "Transcode error", "value": 3 }, { "name": "Transcode success", "value": 70 }],
                    "healthcheck": [{ "name": "Queued", "value": 9 }]
                },
                "video": { "codecs": [{ "name": "hevc", "value": 5 }] },
                "audio": {}
            }
        }))
        .unwrap();
        let stats = map_statistics(data);
        assert_eq!(stats.library_name, "All");
        assert_eq!(stats.failed_transcode_count, 3);
        assert_eq!(stats.staged_transcode_count, 70);
        assert_eq!(stats.failed_health_check_count, 0);
        assert_eq!(stats.staged_health_check_count, 9);
        assert_eq!(stats.total_saved_space, 1_500_000_000.0);
        assert_eq!(stats.video_codecs.len(), 1);
        assert!(stats.audio_codecs.is_empty());
    }

    #[test]
    fn workers_of_all_nodes_are_flattened() {
        let nodes: BTreeMap<String, NodeResponse> = serde_json::from_value(json!({
            "node1": { "workers": {
                "w1": { "_id": "w1", "file": "/a.mkv", "fps": 24.0, "percentage": 50.0, "ETA": "0:10:00",
                        "job": { "type": "transcode" }, "status": "Transcode", "originalfileSizeInGbytes": 2.0,
                        "estSize": 1.0, "lastPluginDetails": { "number": "Tdarr_Plugin_x" } }
            }},
            "node2": { "workers": {
                "w2": { "_id": "w2", "file": "/b.mkv", "fps": 0.0, "percentage": 0.0, "ETA": "0:00:00",
                        "job": { "type": "health" }, "status": "Health", "originalfileSizeInGbytes": 0.5 }
            }}
        }))
        .unwrap();
        let workers = map_workers(nodes);
        assert_eq!(workers.len(), 2);
        assert_eq!(workers[0].step, "Tdarr_Plugin_x");
        assert_eq!(workers[0].original_size, 2_000_000_000.0);
        assert_eq!(workers[0].estimated_size, Some(1_000_000_000.0));
        assert_eq!(workers[1].estimated_size, None);
        assert_eq!(workers[1].step, "");
    }

    #[test]
    fn queue_rows_convert_mb_to_bytes() {
        let table: StatusTable = serde_json::from_value(json!({
            "totalCount": 7,
            "array": [{
                "_id": "f1", "HealthCheck": "Queued", "TranscodeDecisionMaker": "Queued",
                "file": "/x.mkv", "file_size": 1234.5678, "container": "mkv",
                "video_codec_name": "h264", "video_resolution": "1080p"
            }]
        }))
        .unwrap();
        let (items, total) = map_rows(table, TdarrQueueItemType::HealthCheck);
        assert_eq!(total, 7);
        assert_eq!(items[0].file_size, 1_234_567_800.0);
        assert_eq!(items[0].item_type, TdarrQueueItemType::HealthCheck);
    }
}
