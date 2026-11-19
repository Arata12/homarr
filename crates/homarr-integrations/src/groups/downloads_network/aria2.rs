//! aria2 (port of `download-client/aria2/*`), JSON-RPC over HTTP.

use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
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

pub struct Aria2;

const KEYS: &[&str] = &[
    "bittorrent",
    "uploadLength",
    "uploadSpeed",
    "downloadSpeed",
    "totalLength",
    "completedLength",
    "files",
    "status",
    "gid",
];

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Download {
    gid: String,
    status: String,
    total_length: String,
    completed_length: String,
    upload_length: String,
    download_speed: String,
    upload_speed: String,
    files: Vec<DownloadFile>,
    bittorrent: Option<BitTorrent>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DownloadFile {
    path: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BitTorrent {
    info: Option<BitTorrentInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BitTorrentInfo {
    name: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct GlobalStat {
    download_speed: String,
    upload_speed: String,
}

fn number(text: &str) -> f64 {
    text.trim().parse().unwrap_or(0.0)
}

/// `name` is a string, or `{ "utf-8": string }` for non-ASCII names.
fn bittorrent_name(download: &Download) -> Option<String> {
    let name = download.bittorrent.as_ref()?.info.as_ref()?.name.as_ref()?;
    match name {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map.get("utf-8").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn state(status: &str, is_torrent: bool, progress: f64) -> DownloadClientItemState {
    match status {
        // aria2 reports an active torrent as "active" whether it downloads or only seeds, so the state has
        // to come from the progress.
        "active" if is_torrent => {
            if progress >= 1.0 {
                DownloadClientItemState::Seeding
            } else {
                DownloadClientItemState::Leeching
            }
        }
        "active" => DownloadClientItemState::Downloading,
        "waiting" => DownloadClientItemState::Queued,
        "paused" => DownloadClientItemState::Paused,
        "complete" => DownloadClientItemState::Completed,
        "error" => DownloadClientItemState::Failed,
        _ => DownloadClientItemState::Unknown,
    }
}

fn eta_ms(completed: f64, total: f64, speed: f64) -> f64 {
    if speed == 0.0 || completed >= total {
        return 0.0;
    }
    ((total - completed) / speed).floor() * 1000.0
}

fn map_response(
    downloads: Vec<Download>,
    global: &GlobalStat,
    limit: usize,
) -> DownloadClientJobsAndStatus {
    let downloads: Vec<Download> = downloads.into_iter().take(limit).collect();
    let paused = downloads.iter().all(|download| download.status == "paused");
    let items = downloads
        .iter()
        .enumerate()
        .map(|(index, download)| {
            let total = number(&download.total_length);
            let completed = number(&download.completed_length);
            let progress = if total > 0.0 { completed / total } else { 0.0 };
            let is_torrent = download.bittorrent.is_some();
            let name = bittorrent_name(download).unwrap_or_else(|| {
                basename(
                    download
                        .files
                        .first()
                        .map_or("Unknown", |file| file.path.as_str()),
                )
            });
            DownloadClientItem {
                id: download.gid.clone(),
                index: index as f64,
                name,
                item_type: if is_torrent {
                    DownloadClientType::Torrent
                } else {
                    DownloadClientType::Miscellaneous
                },
                size: total,
                sent: Some(number(&download.upload_length)),
                received: None,
                down_speed: Some(number(&download.download_speed)),
                up_speed: Some(number(&download.upload_speed)),
                time: eta_ms(completed, total, number(&download.download_speed)),
                added: None,
                state: state(&download.status, is_torrent, progress),
                progress,
                category: Some(DownloadClientCategory::Many(vec![])),
            }
        })
        .collect();
    DownloadClientJobsAndStatus {
        status: DownloadClientStatus {
            paused,
            rates: DownloadClientRates {
                down: number(&global.download_speed),
                up: Some(number(&global.upload_speed)),
            },
            types: vec![
                DownloadClientType::Torrent,
                DownloadClientType::Miscellaneous,
            ],
        },
        items,
        ratio_items: None,
    }
}

impl Aria2 {
    async fn call(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
        mut params: Vec<Value>,
    ) -> HomarrResult<Value> {
        if let Some(token) = ctx.secret("apiKey") {
            params.insert(0, json!(format!("token:{token}")));
        }
        let url = http::url(ctx, "/jsonrpc", &[])?;
        let req = http::post(rt, url).json(&json!({
            "jsonrpc": "2.0",
            "id": format!("homarr-{}", chrono::Utc::now().timestamp_micros()),
            "method": format!("aria2.{method}"),
            "params": params,
        }));
        let mut body = http::send_json(ctx, req).await?;
        if let Some(error) = body.get("error").filter(|error| !error.is_null()) {
            return Err(HomarrError::Upstream(format!(
                "{} returned an error for {method}: {}",
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

    async fn downloads(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
        params: Vec<Value>,
    ) -> HomarrResult<Vec<Download>> {
        let value = Self::call(rt, ctx, method, params).await?;
        http::from_value(ctx, "the aria2 download list", value)
    }
}

#[async_trait]
impl DownloadClient for Aria2 {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "getVersion", vec![]).await.map(drop)
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        let window = vec![json!(0), json!(limit), json!(KEYS)];
        let (active, waiting, stopped, global) = tokio::join!(
            Self::downloads(rt, ctx, "tellActive", vec![]),
            Self::downloads(rt, ctx, "tellWaiting", window.clone()),
            Self::downloads(rt, ctx, "tellStopped", window),
            Self::call(rt, ctx, "getGlobalStat", vec![]),
        );
        let mut downloads = active?;
        downloads.extend(waiting?);
        downloads.extend(stopped?);
        let global: GlobalStat = http::from_value(ctx, "the aria2 global stats", global?)?;
        Ok(map_response(downloads, &global, limit))
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "pauseAll", vec![]).await.map(drop)
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "unpauseAll", vec![]).await.map(drop)
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "pause", vec![json!(item.id)])
            .await
            .map(drop)
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "unpause", vec![json!(item.id)])
            .await
            .map(drop)
    }

    async fn delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> HomarrResult<()> {
        use DownloadClientItemState as S;
        // aria2 cannot remove downloaded files, so `fromDisk` only decides between a graceful and a forced
        // removal (same as the TS source).
        let active = matches!(
            item.state,
            S::Downloading | S::Leeching | S::Seeding | S::Queued | S::Paused
        );
        let method = if !active {
            "removeDownloadResult"
        } else if from_disk {
            "remove"
        } else {
            "forceRemove"
        };
        Self::call(rt, ctx, method, vec![json!(item.id)])
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_torrent_and_http_downloads() {
        let downloads: Vec<Download> = serde_json::from_value(json!([
            {"gid": "1", "status": "active", "totalLength": "1000", "completedLength": "1000",
             "uploadLength": "50", "downloadSpeed": "0", "uploadSpeed": "9",
             "files": [{"path": "/d/x.iso"}],
             "bittorrent": {"info": {"name": {"utf-8": "Seeded"}}}},
            {"gid": "2", "status": "active", "totalLength": "1000", "completedLength": "250",
             "uploadLength": "0", "downloadSpeed": "50", "uploadSpeed": "0",
             "files": [{"path": "/d/file.zip"}]},
            {"gid": "3", "status": "paused", "totalLength": "0", "completedLength": "0",
             "uploadLength": "0", "downloadSpeed": "0", "uploadSpeed": "0", "files": []}
        ]))
        .unwrap();
        let global = GlobalStat {
            download_speed: "50".into(),
            upload_speed: "9".into(),
        };
        let out = map_response(downloads, &global, 10);
        assert!(!out.status.paused);
        assert_eq!(out.status.rates.down, 50.0);
        assert_eq!(out.items[0].name, "Seeded");
        assert_eq!(out.items[0].item_type, DownloadClientType::Torrent);
        assert_eq!(out.items[0].state, DownloadClientItemState::Seeding);
        assert_eq!(out.items[1].name, "file.zip");
        assert_eq!(out.items[1].state, DownloadClientItemState::Downloading);
        assert_eq!(out.items[1].progress, 0.25);
        assert_eq!(out.items[1].time, 15_000.0);
        assert_eq!(out.items[2].name, "Unknown");
        assert_eq!(out.items[2].progress, 0.0);
        assert_eq!(out.items[2].state, DownloadClientItemState::Paused);
    }

    #[test]
    fn limit_and_all_paused() {
        let downloads: Vec<Download> = serde_json::from_value(json!([
            {"gid": "1", "status": "paused"}, {"gid": "2", "status": "active"}
        ]))
        .unwrap();
        let out = map_response(downloads, &GlobalStat::default(), 1);
        assert_eq!(out.items.len(), 1);
        assert!(out.status.paused);
    }
}
