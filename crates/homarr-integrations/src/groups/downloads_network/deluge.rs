//! Deluge (port of `download-client/deluge/*`; the TS code used `@ctrl/deluge`, i.e. the Deluge web JSON-RPC).

use async_trait::async_trait;
use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::{json, Map, Value};

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

pub struct Deluge;

const FIELDS: &[&str] = &[
    "queue",
    "name",
    "total_wanted",
    "state",
    "progress",
    "download_payload_rate",
    "upload_payload_rate",
    "eta",
    "time_added",
    "total_uploaded",
    "completed_time",
    "label",
];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Torrent {
    queue: f64,
    name: String,
    total_wanted: f64,
    state: String,
    /// 0 to 100.
    progress: f64,
    download_payload_rate: f64,
    upload_payload_rate: f64,
    eta: f64,
    time_added: f64,
    total_uploaded: f64,
    completed_time: f64,
    label: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Stats {
    download_rate: f64,
    upload_rate: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UiUpdate {
    stats: Stats,
    torrents: Map<String, Value>,
}

fn torrent_state(state: &str) -> DownloadClientItemState {
    match state {
        "Queued" | "Checking" | "Allocating" | "Downloading" => DownloadClientItemState::Leeching,
        "Seeding" => DownloadClientItemState::Seeding,
        "Paused" => DownloadClientItemState::Paused,
        _ => DownloadClientItemState::Unknown,
    }
}

fn to_candidate(id: &str, torrent: &Torrent, now_secs: f64) -> Candidate {
    let complete = torrent.progress == 100.0;
    let time = if complete {
        ((torrent.completed_time - now_secs) * 1000.0).min(-1.0)
    } else {
        (torrent.eta * 1000.0).max(0.0)
    };
    let ratio = (torrent.total_wanted > 0.0).then(|| torrent.total_uploaded / torrent.total_wanted);
    Candidate {
        item: DownloadClientItem {
            id: id.to_string(),
            index: torrent.queue,
            name: torrent.name.clone(),
            item_type: DownloadClientType::Torrent,
            size: torrent.total_wanted,
            sent: Some(torrent.total_uploaded),
            received: Some((torrent.total_wanted * (torrent.progress / 100.0)).floor()),
            down_speed: (!complete).then_some(torrent.download_payload_rate),
            up_speed: Some(torrent.upload_payload_rate),
            time,
            added: Some(torrent.time_added * 1000.0),
            state: torrent_state(&torrent.state),
            progress: torrent.progress / 100.0,
            category: torrent.label.clone().map(DownloadClientCategory::One),
        },
        ratio,
    }
}

fn map_response(
    update: UiUpdate,
    limit: usize,
    selection: Option<&DownloadClientSelection>,
    now_secs: f64,
) -> HomarrResult<DownloadClientJobsAndStatus> {
    let mut candidates = Vec::with_capacity(update.torrents.len());
    for (id, value) in &update.torrents {
        let torrent: Torrent = serde_json::from_value(value.clone()).map_err(|err| {
            HomarrError::Upstream(format!("Failed to parse Deluge torrent {id}: {err}"))
        })?;
        candidates.push(to_candidate(id, &torrent, now_secs));
    }
    let paused = candidates
        .iter()
        .all(|candidate| candidate.item.state == DownloadClientItemState::Paused);
    let status = DownloadClientStatus {
        paused,
        rates: DownloadClientRates {
            down: update.stats.download_rate.floor(),
            up: Some(update.stats.upload_rate.floor()),
        },
        types: vec![DownloadClientType::Torrent],
    };
    let ratio_items = selection.map(|_| {
        let all: Vec<DownloadClientItem> = candidates.iter().map(|c| c.item.clone()).collect();
        selection::ratio_items(&all, limit)
    });
    let items = selection::select(candidates, limit, selection);
    Ok(DownloadClientJobsAndStatus {
        status,
        items,
        ratio_items,
    })
}

impl Deluge {
    async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
        let password = ctx.require_secret("password")?;
        let url = http::url(ctx, "/json", &[])?;
        let req = http::post(rt, url)
            .json(&json!({ "method": "auth.login", "params": [password], "id": 1 }));
        let resp = http::send(ctx, req).await?;
        let cookie = http::cookie_header(&resp);
        let body = http::read_json(ctx, resp).await?;
        let ok = body.get("result").and_then(Value::as_bool) == Some(true)
            && body.get("error").is_none_or(Value::is_null);
        match (ok, cookie) {
            (true, Some(cookie)) => Ok(cookie),
            _ => Err(HomarrError::Upstream(format!(
                "Deluge authentication failed for {}. Check the password",
                ctx.name
            ))),
        }
    }

    async fn rpc(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        method: &str,
        params: Value,
    ) -> HomarrResult<Value> {
        for attempt in 0..2 {
            let cookie = match rt.sessions.get::<String>(ctx) {
                Some(cookie) => cookie,
                None => {
                    let cookie = Self::login(rt, ctx).await?;
                    rt.sessions.set(ctx, &cookie, None);
                    cookie
                }
            };
            let url = http::url(ctx, "/json", &[])?;
            let req = http::post(rt, url)
                .header(reqwest::header::COOKIE, cookie)
                .json(&json!({ "method": method, "params": params, "id": 1 }));
            let resp = http::send_raw(ctx, req).await?;
            let unauthorized = matches!(resp.status().as_u16(), 401 | 403);
            if unauthorized && attempt == 0 {
                rt.sessions.clear(ctx);
                continue;
            }
            let mut body = http::read_json(ctx, crate::error::check_status(ctx, resp)?).await?;
            if let Some(error) = body.get("error").filter(|error| !error.is_null()) {
                // Code 1 = "Not authenticated": the stored session expired.
                if attempt == 0 && error.get("code").and_then(Value::as_i64) == Some(1) {
                    rt.sessions.clear(ctx);
                    continue;
                }
                return Err(HomarrError::Upstream(format!(
                    "{} returned an error for {method}: {}",
                    ctx.name,
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )));
            }
            return Ok(body
                .get_mut("result")
                .map(Value::take)
                .unwrap_or(Value::Null));
        }
        Err(HomarrError::Upstream(format!(
            "Deluge authentication failed for {}",
            ctx.name
        )))
    }

    async fn update_ui(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<UiUpdate> {
        let result = Self::rpc(rt, ctx, "web.update_ui", json!([FIELDS, {}])).await?;
        http::from_value(ctx, "the Deluge torrent list", result)
    }

    async fn torrent_ids(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<String>> {
        Ok(Self::update_ui(rt, ctx)
            .await?
            .torrents
            .keys()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl DownloadClient for Deluge {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let cookie = Self::login(rt, ctx).await?;
        rt.sessions.set(ctx, &cookie, None);
        Ok(())
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        // There is no way to limit the number of returned torrents server side.
        let update = Self::update_ui(rt, ctx).await?;
        map_response(update, limit, selection, Utc::now().timestamp() as f64)
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let ids = Self::torrent_ids(rt, ctx).await?;
        Self::rpc(rt, ctx, "core.pause_torrent", json!([ids]))
            .await
            .map(drop)
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let ids = Self::torrent_ids(rt, ctx).await?;
        Self::rpc(rt, ctx, "core.resume_torrent", json!([ids]))
            .await
            .map(drop)
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::rpc(rt, ctx, "core.pause_torrent", json!([[item.id]]))
            .await
            .map(drop)
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::rpc(rt, ctx, "core.resume_torrent", json!([[item.id]]))
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
        Self::rpc(rt, ctx, "core.remove_torrent", json!([item.id, from_disk]))
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update() -> UiUpdate {
        serde_json::from_value(json!({
            "stats": {"download_rate": 1500.7, "upload_rate": 20.2},
            "torrents": {
                "h1": {"queue": 0, "name": "a", "total_wanted": 1000, "state": "Downloading",
                       "progress": 50, "download_payload_rate": 100, "upload_payload_rate": 5,
                       "eta": 12, "time_added": 100, "total_uploaded": 10, "completed_time": 0,
                       "label": "tv"},
                "h2": {"queue": 1, "name": "b", "total_wanted": 2000, "state": "Paused",
                       "progress": 100, "download_payload_rate": 0, "upload_payload_rate": 0,
                       "eta": 0, "time_added": 100, "total_uploaded": 4000, "completed_time": 900}
            }
        }))
        .unwrap()
    }

    #[test]
    fn maps_torrents() {
        let out = map_response(update(), 10, None, 1000.0).unwrap();
        assert!(!out.status.paused);
        assert_eq!(out.status.rates.down, 1500.0);
        assert_eq!(out.status.rates.up, Some(20.0));
        assert!(out.ratio_items.is_none());
        let a = out.items.iter().find(|i| i.id == "h1").unwrap();
        assert_eq!(a.progress, 0.5);
        assert_eq!(a.time, 12_000.0);
        assert_eq!(a.state, DownloadClientItemState::Leeching);
        assert_eq!(a.down_speed, Some(100.0));
        assert_eq!(a.received, Some(500.0));
        let b = out.items.iter().find(|i| i.id == "h2").unwrap();
        assert_eq!(b.time, -100_000.0);
        assert_eq!(b.down_speed, None);
        assert_eq!(b.category, None);
    }

    #[test]
    fn selection_adds_ratio_items() {
        let selection = DownloadClientSelection {
            sort: "name".into(),
            descending: false,
            category_filter: vec![],
            filter_is_whitelist: false,
            show_completed_torrent: false,
            active_torrent_threshold: 0.0,
        };
        let out = map_response(update(), 10, Some(&selection), 1000.0).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.ratio_items.unwrap().len(), 2);
    }

    #[test]
    fn maps_states() {
        assert_eq!(torrent_state("Seeding"), DownloadClientItemState::Seeding);
        assert_eq!(torrent_state("Error"), DownloadClientItemState::Unknown);
    }
}
