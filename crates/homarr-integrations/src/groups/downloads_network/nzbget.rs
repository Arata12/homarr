//! NZBGet (port of `download-client/nzbget/*`).

use async_trait::async_trait;
use chrono::Utc;
use homarr_common::HomarrResult;
use serde::Deserialize;
use serde_json::{json, Value};

use super::http;
use super::DownloadClient;
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientItemState,
    DownloadClientJobsAndStatus, DownloadClientRates, DownloadClientSelection,
    DownloadClientStatus, DownloadClientType,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct NzbGet;

const TWO_POW_32: f64 = 4_294_967_296.0;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    download_paused: bool,
    download_rate: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Group {
    status: String,
    #[serde(rename = "NZBID")]
    nzb_id: i64,
    max_priority: f64,
    #[serde(rename = "NZBName")]
    nzb_name: String,
    file_size_lo: f64,
    file_size_hi: f64,
    active_downloads: f64,
    remaining_size_lo: f64,
    remaining_size_hi: f64,
    download_time_sec: f64,
    category: String,
    #[serde(rename = "DownloadedSizeMB")]
    downloaded_size_mb: f64,
    #[serde(rename = "FileSizeMB")]
    file_size_mb: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HistoryEntry {
    script_status: String,
    #[serde(rename = "NZBID")]
    nzb_id: i64,
    name: String,
    file_size_lo: f64,
    file_size_hi: f64,
    history_time: f64,
    download_time_sec: f64,
    category: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FileEntry {
    #[serde(rename = "ID")]
    id: i64,
}

fn queue_state(status: &str) -> DownloadClientItemState {
    match status {
        "QUEUED" => DownloadClientItemState::Queued,
        "PAUSED" => DownloadClientItemState::Paused,
        _ => DownloadClientItemState::Downloading,
    }
}

fn history_state(status: &str) -> DownloadClientItemState {
    match status {
        "FAILURE" => DownloadClientItemState::Failed,
        "SUCCESS" => DownloadClientItemState::Completed,
        _ => DownloadClientItemState::Processing,
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn map_response(
    status: Status,
    queue: Vec<Group>,
    history: Vec<HistoryEntry>,
    limit: usize,
    now_secs: f64,
) -> DownloadClientJobsAndStatus {
    let kind = DownloadClientType::Usenet;
    let rate = status.download_rate;
    let queue_items = queue.into_iter().map(|file| {
        let time = (file.remaining_size_lo + file.remaining_size_hi * TWO_POW_32) / (rate / 1000.0);
        DownloadClientItem {
            item_type: kind,
            id: file.nzb_id.to_string(),
            index: file.max_priority,
            name: file.nzb_name,
            size: file.file_size_lo + file.file_size_hi * TWO_POW_32,
            sent: None,
            received: None,
            down_speed: Some(if file.active_downloads > 0.0 {
                rate
            } else {
                0.0
            }),
            up_speed: None,
            time: finite_or(time, 0.0),
            added: Some((now_secs - file.download_time_sec) * 1000.0),
            state: queue_state(&file.status),
            progress: finite_or(file.downloaded_size_mb / file.file_size_mb, 0.0),
            category: Some(DownloadClientCategory::One(file.category)),
        }
    });
    let history_items = history
        .into_iter()
        .enumerate()
        .map(|(index, file)| DownloadClientItem {
            item_type: kind,
            id: file.nzb_id.to_string(),
            index: index as f64,
            name: file.name,
            size: file.file_size_lo + file.file_size_hi * TWO_POW_32,
            sent: None,
            received: None,
            down_speed: None,
            up_speed: None,
            time: (now_secs - file.history_time) * 1000.0,
            added: Some((file.history_time - file.download_time_sec) * 1000.0),
            state: history_state(&file.script_status),
            progress: 1.0,
            category: Some(DownloadClientCategory::One(file.category)),
        });
    DownloadClientJobsAndStatus {
        status: DownloadClientStatus {
            paused: status.download_paused,
            rates: DownloadClientRates {
                down: rate,
                up: None,
            },
            types: vec![kind],
        },
        items: queue_items.chain(history_items).take(limit).collect(),
        ratio_items: None,
    }
}

impl NzbGet {
    async fn call(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
        params: Value,
    ) -> HomarrResult<Value> {
        let username = ctx.require_secret("username")?;
        let password = ctx.require_secret("password")?;
        let url = http::url(ctx, "/jsonrpc", &[])?;
        let req = http::post(rt, url)
            .header(
                reqwest::header::AUTHORIZATION,
                http::basic_auth_value(username, password),
            )
            .json(&json!({ "method": method, "params": params }));
        let mut body = http::send_json(ctx, req).await?;
        if let Some(error) = body.get("error").filter(|error| !error.is_null()) {
            return Err(homarr_common::HomarrError::Upstream(format!(
                "{} returned an error for '{method}': {}",
                ctx.name,
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            )));
        }
        Ok(body
            .get_mut("result")
            .map(Value::take)
            .unwrap_or(Value::Null))
    }

    async fn edit_queue(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        command: &str,
        ids: Vec<i64>,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "editqueue", json!([command, "", ids]))
            .await
            .map(drop)
    }

    fn numeric_id(item: &DownloadClientItem) -> HomarrResult<i64> {
        item.id.parse::<i64>().map_err(|_| {
            homarr_common::HomarrError::BadRequest(format!("Invalid NZBGet item id '{}'", item.id))
        })
    }
}

#[async_trait]
impl DownloadClient for NzbGet {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "version", json!([])).await.map(drop)
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        let (queue, history, status) = tokio::join!(
            Self::call(rt, ctx, "listgroups", json!([])),
            Self::call(rt, ctx, "history", json!([])),
            Self::call(rt, ctx, "status", json!([])),
        );
        let queue: Vec<Group> = http::from_value(ctx, "the NZBGet queue", queue?)?;
        let history: Vec<HistoryEntry> = http::from_value(ctx, "the NZBGet history", history?)?;
        let status: Status = http::from_value(ctx, "the NZBGet status", status?)?;
        Ok(map_response(
            status,
            queue,
            history,
            limit,
            Utc::now().timestamp() as f64,
        ))
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "pausedownload", json!([]))
            .await
            .map(drop)
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "resumedownload", json!([]))
            .await
            .map(drop)
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::edit_queue(rt, ctx, "GroupPause", vec![Self::numeric_id(item)?]).await
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::edit_queue(rt, ctx, "GroupResume", vec![Self::numeric_id(item)?]).await
    }

    async fn delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> HomarrResult<()> {
        let id = Self::numeric_id(item)?;
        if from_disk {
            let files = Self::call(rt, ctx, "listfiles", json!([0, 0, id])).await?;
            let files: Vec<FileEntry> = http::from_value(ctx, "the NZBGet file list", files)?;
            Self::edit_queue(
                rt,
                ctx,
                "FileDelete",
                files.into_iter().map(|f| f.id).collect(),
            )
            .await?;
        }
        // Finished items live in the history; the TS source had the two commands swapped.
        let command = if item.progress == 1.0 {
            "HistoryFinalDelete"
        } else {
            "GroupFinalDelete"
        };
        Self::edit_queue(rt, ctx, command, vec![id]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_queue_and_history() {
        let status: Status =
            serde_json::from_value(json!({"DownloadPaused": true, "DownloadRate": 2000})).unwrap();
        let queue: Vec<Group> = serde_json::from_value(json!([{
            "Status": "DOWNLOADING", "NZBID": 5, "MaxPriority": 0, "NZBName": "x",
            "FileSizeLo": 100, "FileSizeHi": 1, "ActiveDownloads": 1,
            "RemainingSizeLo": 4000, "RemainingSizeHi": 0, "DownloadTimeSec": 10,
            "Category": "tv", "DownloadedSizeMB": 1, "FileSizeMB": 4
        }]))
        .unwrap();
        let history: Vec<HistoryEntry> = serde_json::from_value(json!([{
            "ScriptStatus": "SUCCESS", "NZBID": 6, "Name": "y", "FileSizeLo": 1, "FileSizeHi": 0,
            "HistoryTime": 900, "DownloadTimeSec": 100, "Category": ""
        }]))
        .unwrap();
        let out = map_response(status, queue, history, 10, 1000.0);
        assert!(out.status.paused);
        assert_eq!(out.items[0].id, "5");
        assert_eq!(out.items[0].size, 100.0 + TWO_POW_32);
        assert_eq!(out.items[0].time, 2000.0);
        assert_eq!(out.items[0].progress, 0.25);
        assert_eq!(out.items[0].down_speed, Some(2000.0));
        assert_eq!(out.items[0].added, Some(990_000.0));
        assert_eq!(out.items[1].state, DownloadClientItemState::Completed);
        assert_eq!(out.items[1].time, 100_000.0);
        assert_eq!(out.items[1].added, Some(800_000.0));
    }

    #[test]
    fn zero_rate_gives_zero_eta() {
        let status = Status {
            download_paused: false,
            download_rate: 0.0,
        };
        let queue: Vec<Group> = serde_json::from_value(json!([{
            "Status": "QUEUED", "NZBID": 1, "MaxPriority": 0, "NZBName": "x",
            "FileSizeLo": 0, "FileSizeHi": 0, "ActiveDownloads": 0,
            "RemainingSizeLo": 0, "RemainingSizeHi": 0, "DownloadTimeSec": 0,
            "Category": "", "DownloadedSizeMB": 0, "FileSizeMB": 0
        }]))
        .unwrap();
        let out = map_response(status, queue, vec![], 10, 0.0);
        assert_eq!(out.items[0].time, 0.0);
        assert_eq!(out.items[0].progress, 0.0);
        assert_eq!(out.items[0].state, DownloadClientItemState::Queued);
    }
}
