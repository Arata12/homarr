//! Transmission (port of `download-client/transmission/*`; the TS code used `@ctrl/transmission`).

use async_trait::async_trait;
use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::{json, Value};

use super::http;
use super::selection::{self, Candidate};
use super::DownloadClient;
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientItemState,
    DownloadClientJobsAndStatus, DownloadClientRates, DownloadClientSelection,
    DownloadClientStatus, DownloadClientType,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct Transmission;

const SESSION_HEADER: &str = "X-Transmission-Session-Id";

const FIELDS: &[&str] = &[
    "hashString",
    "name",
    "totalSize",
    "percentDone",
    "status",
    "rateDownload",
    "rateUpload",
    "eta",
    "addedDate",
    "doneDate",
    "queuePosition",
    "uploadedEver",
    "downloadedEver",
    "labels",
];

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Torrent {
    hash_string: String,
    name: String,
    total_size: f64,
    percent_done: f64,
    status: i64,
    rate_download: f64,
    rate_upload: f64,
    eta: f64,
    added_date: f64,
    done_date: f64,
    queue_position: f64,
    uploaded_ever: f64,
    downloaded_ever: f64,
    labels: Vec<String>,
}

fn torrent_state(status: i64) -> DownloadClientItemState {
    match status {
        0 => DownloadClientItemState::Paused,
        1 | 3 => DownloadClientItemState::Stalled,
        2 | 4 => DownloadClientItemState::Leeching,
        5 | 6 => DownloadClientItemState::Seeding,
        _ => DownloadClientItemState::Unknown,
    }
}

fn to_candidate(torrent: &Torrent, now_ms: f64) -> Candidate {
    let complete = torrent.percent_done == 1.0;
    let time = if complete {
        (torrent.done_date * 1000.0 - now_ms).min(-1.0)
    } else {
        (torrent.eta * 1000.0).max(0.0)
    };
    Candidate {
        item: DownloadClientItem {
            id: torrent.hash_string.clone(),
            index: torrent.queue_position,
            name: torrent.name.clone(),
            item_type: DownloadClientType::Torrent,
            size: torrent.total_size,
            sent: Some(torrent.uploaded_ever),
            received: Some(torrent.downloaded_ever),
            down_speed: (!complete).then_some(torrent.rate_download),
            up_speed: Some(torrent.rate_upload),
            time,
            added: Some(torrent.added_date * 1000.0),
            state: torrent_state(torrent.status),
            progress: torrent.percent_done,
            category: Some(DownloadClientCategory::Many(torrent.labels.clone())),
        },
        ratio: (torrent.total_size > 0.0).then(|| torrent.uploaded_ever / torrent.total_size),
    }
}

fn map_response(
    torrents: &[Torrent],
    limit: usize,
    selection: Option<&DownloadClientSelection>,
    now_ms: f64,
) -> DownloadClientJobsAndStatus {
    let candidates: Vec<Candidate> = torrents.iter().map(|t| to_candidate(t, now_ms)).collect();
    let status = DownloadClientStatus {
        paused: torrents
            .iter()
            .all(|t| torrent_state(t.status) == DownloadClientItemState::Paused),
        rates: DownloadClientRates {
            down: torrents.iter().map(|t| t.rate_download).sum(),
            up: Some(torrents.iter().map(|t| t.rate_upload).sum()),
        },
        types: vec![DownloadClientType::Torrent],
    };
    let ratio_items = selection.map(|_| {
        let all: Vec<DownloadClientItem> = candidates.iter().map(|c| c.item.clone()).collect();
        selection::ratio_items(&all, limit)
    });
    DownloadClientJobsAndStatus {
        status,
        items: selection::select(candidates, limit, selection),
        ratio_items,
    }
}

impl Transmission {
    /// One RPC call with the `X-Transmission-Session-Id` handshake: a 409 answer carries the id to use, which is
    /// cached for later calls.
    async fn rpc(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
        arguments: Value,
    ) -> HomarrResult<Value> {
        let username = ctx.require_secret("username")?;
        let password = ctx.require_secret("password")?;
        let payload = json!({ "method": method, "arguments": arguments });
        let mut session_id = rt.sessions.get::<String>(ctx);
        for _ in 0..2 {
            let url = http::url(ctx, "/transmission/rpc", &[])?;
            let mut req = http::post(rt, url)
                .header(
                    reqwest::header::AUTHORIZATION,
                    http::basic_auth_value(username, password),
                )
                .json(&payload);
            if let Some(id) = &session_id {
                req = req.header(SESSION_HEADER, id);
            }
            let resp = http::send_raw(ctx, req).await?;
            if resp.status().as_u16() == 409 {
                let id = resp
                    .headers()
                    .get(SESSION_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        HomarrError::Upstream(format!(
                            "{} did not return a Transmission session id",
                            ctx.name
                        ))
                    })?;
                rt.sessions.set(ctx, &id, None);
                session_id = Some(id);
                continue;
            }
            let body = http::read_json(ctx, crate::error::check_status(ctx, resp)?).await?;
            let result = body.get("result").and_then(Value::as_str).unwrap_or("");
            if result != "success" {
                return Err(HomarrError::Upstream(format!(
                    "{} reported '{result}' for {method}",
                    ctx.name
                )));
            }
            return Ok(body.get("arguments").cloned().unwrap_or(Value::Null));
        }
        Err(HomarrError::Upstream(format!(
            "{} kept rejecting the Transmission session id",
            ctx.name
        )))
    }

    async fn list(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Vec<Torrent>> {
        let arguments = Self::rpc(rt, ctx, "torrent-get", json!({ "fields": FIELDS })).await?;
        let torrents = arguments
            .get("torrents")
            .cloned()
            .unwrap_or(Value::Array(vec![]));
        http::from_value(ctx, "the Transmission torrent list", torrents)
    }

    async fn for_all(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
    ) -> HomarrResult<()> {
        let ids: Vec<String> = Self::list(rt, ctx)
            .await?
            .into_iter()
            .map(|t| t.hash_string)
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        Self::rpc(rt, ctx, method, json!({ "ids": ids }))
            .await
            .map(drop)
    }
}

#[async_trait]
impl DownloadClient for Transmission {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::rpc(rt, ctx, "session-get", json!({})).await.map(drop)
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        // There is no way to limit the number of returned torrents server side.
        let torrents = Self::list(rt, ctx).await?;
        Ok(map_response(
            &torrents,
            limit,
            selection,
            Utc::now().timestamp_millis() as f64,
        ))
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::for_all(rt, ctx, "torrent-stop").await
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::for_all(rt, ctx, "torrent-start").await
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::rpc(rt, ctx, "torrent-stop", json!({ "ids": [item.id] }))
            .await
            .map(drop)
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::rpc(rt, ctx, "torrent-start", json!({ "ids": [item.id] }))
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
        Self::rpc(
            rt,
            ctx,
            "torrent-remove",
            json!({ "ids": [item.id], "delete-local-data": from_disk }),
        )
        .await
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrents() -> Vec<Torrent> {
        serde_json::from_value(json!([
            {"hashString": "a", "name": "A", "totalSize": 1000, "percentDone": 0.5, "status": 4,
             "rateDownload": 100, "rateUpload": 10, "eta": 30, "addedDate": 5, "doneDate": 0,
             "queuePosition": 0, "uploadedEver": 20, "downloadedEver": 500, "labels": ["tv"]},
            {"hashString": "b", "name": "B", "totalSize": 2000, "percentDone": 1, "status": 6,
             "rateDownload": 0, "rateUpload": 40, "eta": -1, "addedDate": 6, "doneDate": 100,
             "queuePosition": 1, "uploadedEver": 4000, "downloadedEver": 2000, "labels": []}
        ]))
        .unwrap()
    }

    #[test]
    fn maps_status_and_items() {
        let out = map_response(&torrents(), 10, None, 200_000.0);
        assert!(!out.status.paused);
        assert_eq!(out.status.rates.down, 100.0);
        assert_eq!(out.status.rates.up, Some(50.0));
        assert_eq!(out.items[0].time, 30_000.0);
        assert_eq!(out.items[0].state, DownloadClientItemState::Leeching);
        assert_eq!(out.items[0].down_speed, Some(100.0));
        assert_eq!(out.items[1].time, -100_000.0);
        assert_eq!(out.items[1].down_speed, None);
        assert_eq!(out.items[1].state, DownloadClientItemState::Seeding);
    }

    #[test]
    fn all_paused_and_empty_report_paused() {
        assert!(map_response(&[], 10, None, 0.0).status.paused);
        let mut t = torrents();
        for torrent in &mut t {
            torrent.status = 0;
        }
        assert!(map_response(&t, 10, None, 0.0).status.paused);
    }

    #[test]
    fn selection_filters_by_label_blacklist() {
        let selection = DownloadClientSelection {
            sort: "name".into(),
            descending: true,
            category_filter: vec!["tv".into()],
            filter_is_whitelist: false,
            show_completed_torrent: true,
            active_torrent_threshold: 0.0,
        };
        let out = map_response(&torrents(), 10, Some(&selection), 0.0);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].id, "b");
        assert_eq!(out.ratio_items.unwrap().len(), 2);
    }
}
