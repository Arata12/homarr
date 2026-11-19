//! qBittorrent (port of `download-client/qbittorrent/*`; the TS code used `@ctrl/qbittorrent`).
//!
//! Session handling: username/password log in once (`SID` cookie) and the cookie is kept in `rt.sessions`
//! together with the detected major version (v5 renamed pause/resume to stop/start). An API key is sent as a
//! bearer token. A 401/403 answer drops the stored session and retries once.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http;
use super::DownloadClient;
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientItemState,
    DownloadClientJobsAndStatus, DownloadClientRates, DownloadClientRatioItem,
    DownloadClientSelection, DownloadClientStatus, DownloadClientType,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct QBittorrent;

const SESSION_TTL: Duration = Duration::from_secs(600);
const MAX_PAGES: usize = 100;
const PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    cookie: Option<String>,
    v5: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Torrent {
    hash: String,
    priority: f64,
    name: String,
    size: f64,
    uploaded: f64,
    dlspeed: f64,
    upspeed: f64,
    progress: f64,
    completion_on: f64,
    eta: f64,
    added_on: f64,
    state: String,
    category: String,
    ratio: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TransferInfo {
    dl_info_speed: f64,
    up_info_speed: f64,
}

fn torrent_state(state: &str) -> DownloadClientItemState {
    match state {
        "allocating" | "checkingDL" | "downloading" | "forcedDL" | "forcedMetaDL" | "metaDL"
        | "queuedDL" | "queuedForChecking" => DownloadClientItemState::Leeching,
        "checkingUP" | "forcedUP" | "queuedUP" | "uploading" | "stalledUP" => {
            DownloadClientItemState::Seeding
        }
        "pausedDL" | "pausedUP" | "stoppedDL" | "stoppedUP" => DownloadClientItemState::Paused,
        "stalledDL" => DownloadClientItemState::Stalled,
        _ => DownloadClientItemState::Unknown,
    }
}

fn map_item(torrent: &Torrent, now_ms: f64) -> DownloadClientItem {
    let complete = torrent.progress == 1.0;
    let time = if complete {
        (torrent.completion_on * 1000.0 - now_ms).min(-1.0)
    } else if torrent.eta == 8_640_000.0 {
        0.0
    } else {
        (torrent.eta * 1000.0).max(0.0)
    };
    DownloadClientItem {
        id: torrent.hash.clone(),
        index: torrent.priority,
        name: torrent.name.clone(),
        item_type: DownloadClientType::Torrent,
        size: torrent.size,
        sent: Some(torrent.uploaded),
        received: None,
        down_speed: (!complete).then_some(torrent.dlspeed),
        up_speed: Some(torrent.upspeed),
        time,
        added: Some(torrent.added_on * 1000.0),
        state: torrent_state(&torrent.state),
        progress: torrent.progress,
        category: Some(DownloadClientCategory::One(torrent.category.clone())),
    }
}

fn status_of(torrents: &[Torrent]) -> DownloadClientStatus {
    DownloadClientStatus {
        paused: torrents
            .iter()
            .all(|t| torrent_state(&t.state) == DownloadClientItemState::Paused),
        rates: DownloadClientRates {
            down: torrents.iter().map(|t| t.dlspeed).sum(),
            up: Some(torrents.iter().map(|t| t.upspeed).sum()),
        },
        types: vec![DownloadClientType::Torrent],
    }
}

/// Remote sort field for the selection sorts that mean the same in qBittorrent and the widget.
fn remote_sort_field(sort: &str) -> Option<&'static str> {
    Some(match sort {
        "name" => "name",
        "progress" => "progress",
        "size" => "size",
        "added" => "added_on",
        "index" => "priority",
        "upSpeed" => "upspeed",
        "downSpeed" => "dlspeed",
        "sent" => "uploaded",
        _ => return None,
    })
}

/// Client side part of the selection (category filter and completed-torrent rules).
fn accepts(selection: &DownloadClientSelection, torrent: &Torrent) -> bool {
    if !selection.category_filter.is_empty() {
        let matches = selection.category_filter.contains(&torrent.category);
        if selection.filter_is_whitelist != matches {
            return false;
        }
    }
    if torrent.progress >= 1.0 {
        if !selection.show_completed_torrent {
            return false;
        }
        if torrent.upspeed < selection.active_torrent_threshold * 1024.0 {
            return false;
        }
    }
    true
}

impl QBittorrent {
    async fn establish(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Session> {
        let mut session = Session {
            cookie: None,
            v5: false,
        };
        if ctx.secret("apiKey").is_none() {
            let username = ctx.require_secret("username")?;
            let password = ctx.require_secret("password")?;
            let url = http::url(ctx, "/api/v2/auth/login", &[])?;
            let req = http::post(rt, url).form(&[("username", username), ("password", password)]);
            let resp = http::send(ctx, req).await?;
            let cookie = http::cookie_header(&resp);
            let body = http::read_text(ctx, resp).await?;
            match cookie {
                Some(cookie) if body.trim() != "Fails." => session.cookie = Some(cookie),
                _ => {
                    return Err(HomarrError::Upstream(format!(
                        "{} rejected the credentials. Check the username and password",
                        ctx.name
                    )))
                }
            }
        }
        // Populate the version so the right endpoints (stop/start vs pause/resume) are used.
        let url = http::url(ctx, "/api/v2/app/version", &[])?;
        let req = Self::authorize(http::get(rt, url), ctx, &session);
        let version = http::read_text(ctx, http::send(ctx, req).await?).await?;
        session.v5 = parse_major_version(&version).is_some_and(|major| major >= 5);
        Ok(session)
    }

    fn authorize(
        req: reqwest::RequestBuilder,
        ctx: &IntegrationContext,
        session: &Session,
    ) -> reqwest::RequestBuilder {
        if let Some(key) = ctx.secret("apiKey") {
            req.bearer_auth(key)
        } else if let Some(cookie) = &session.cookie {
            req.header(reqwest::header::COOKIE, cookie)
        } else {
            req
        }
    }

    async fn session(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Session> {
        if let Some(session) = rt.sessions.get::<Session>(ctx) {
            return Ok(session);
        }
        let session = Self::establish(rt, ctx).await?;
        rt.sessions.set(ctx, &session, Some(SESSION_TTL));
        Ok(session)
    }

    /// Executes a request with the current session; returns the session used (for the version).
    async fn request(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        path: &str,
        query: &[(&str, &str)],
        form: Option<&[(&str, &str)]>,
    ) -> HomarrResult<(Session, Response)> {
        for attempt in 0..2 {
            let session = Self::session(rt, ctx).await?;
            let url = http::url(ctx, path, query)?;
            let req = match form {
                Some(form) => http::post(rt, url).form(form),
                None => http::get(rt, url),
            };
            let resp = http::send_raw(ctx, Self::authorize(req, ctx, &session)).await?;
            if matches!(resp.status().as_u16(), 401 | 403) && attempt == 0 {
                rt.sessions.clear(ctx);
                continue;
            }
            return Ok((session, crate::error::check_status(ctx, resp)?));
        }
        Err(HomarrError::Upstream(format!(
            "{} rejected the session",
            ctx.name
        )))
    }

    async fn list(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        query: &[(&str, &str)],
    ) -> HomarrResult<Vec<Torrent>> {
        let (_, resp) = Self::request(rt, ctx, "/api/v2/torrents/info", query, None).await?;
        let value = http::read_json(ctx, resp).await?;
        http::from_value(ctx, "the qBittorrent torrent list", value)
    }

    async fn selected(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<Vec<Torrent>> {
        let Some(selection) = selection else {
            return Self::list(rt, ctx, &[("limit", &limit.to_string())]).await;
        };
        let category = (selection.filter_is_whitelist && selection.category_filter.len() == 1)
            .then(|| selection.category_filter[0].clone());
        // Bound every response, retained items and total work even when nearly the whole queue is hidden.
        let mut page_size = limit.min(PAGE_SIZE);
        if !selection.category_filter.is_empty()
            || !selection.show_completed_torrent
            || selection.active_torrent_threshold > 0.0
        {
            page_size = PAGE_SIZE;
        }
        let sort = remote_sort_field(&selection.sort);
        let reverse = if selection.descending {
            "true"
        } else {
            "false"
        };
        let mut torrents: Vec<Torrent> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for page in 0..MAX_PAGES {
            if sort.is_some() && torrents.len() >= limit {
                break;
            }
            let previous_seen = seen.len();
            let limit_text = page_size.to_string();
            let offset_text = (page * page_size).to_string();
            let mut query: Vec<(&str, &str)> =
                vec![("limit", &limit_text), ("offset", &offset_text)];
            if let Some(sort) = sort {
                query.push(("sort", sort));
                query.push(("reverse", reverse));
            }
            if let Some(category) = &category {
                query.push(("category", category));
            }
            let batch = Self::list(rt, ctx, &query).await?;
            let batch_len = batch.len();
            for torrent in batch {
                if !seen.insert(torrent.hash.clone()) {
                    continue;
                }
                if !accepts(selection, &torrent) {
                    continue;
                }
                torrents.push(torrent);
                if sort.is_some() && torrents.len() >= limit {
                    break;
                }
            }
            // qBittorrent 4 wraps out-of-range offsets back to the first page.
            if batch_len < page_size || seen.len() == previous_seen {
                break;
            }
        }
        if sort.is_none() {
            let now_ms = Utc::now().timestamp_millis() as f64;
            let candidates: Vec<super::selection::Candidate> = torrents
                .iter()
                .map(|torrent| super::selection::Candidate {
                    item: map_item(torrent, now_ms),
                    ratio: Some(torrent.ratio),
                })
                .collect();
            let chosen = super::selection::select(candidates, limit, Some(selection));
            let mut by_hash: std::collections::HashMap<String, Torrent> = torrents
                .into_iter()
                .map(|torrent| (torrent.hash.clone(), torrent))
                .collect();
            return Ok(chosen
                .into_iter()
                .filter_map(|item| by_hash.remove(&item.id))
                .collect());
        }
        Ok(torrents)
    }

    async fn ratio_items(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
    ) -> HomarrResult<Vec<DownloadClientRatioItem>> {
        // Preserve the original unfiltered limited population.
        let mut items: Vec<DownloadClientRatioItem> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for page in 0..MAX_PAGES {
            if items.len() >= limit {
                break;
            }
            let page_size = (limit - items.len()).min(PAGE_SIZE);
            let limit_text = page_size.to_string();
            let offset_text = (page * PAGE_SIZE).to_string();
            let batch =
                Self::list(rt, ctx, &[("limit", &limit_text), ("offset", &offset_text)]).await?;
            let previous = items.len();
            let batch_len = batch.len();
            for torrent in batch {
                if !seen.insert(torrent.hash.clone()) {
                    continue;
                }
                items.push(DownloadClientRatioItem {
                    sent: Some(torrent.uploaded),
                    size: torrent.size,
                    progress: torrent.progress,
                    category: Some(DownloadClientCategory::One(torrent.category)),
                });
                if items.len() >= limit {
                    break;
                }
            }
            if batch_len < page_size || items.len() == previous {
                break;
            }
        }
        Ok(items)
    }

    async fn torrents_action(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        action: &str,
        hashes: &str,
    ) -> HomarrResult<()> {
        // v5 renamed pause/resume to stop/start.
        let session = Self::session(rt, ctx).await?;
        let endpoint = match (action, session.v5) {
            ("pause", true) => "stop",
            ("resume", true) => "start",
            (other, _) => other,
        };
        let path = format!("/api/v2/torrents/{endpoint}");
        Self::request(rt, ctx, &path, &[], Some(&[("hashes", hashes)]))
            .await
            .map(drop)
    }
}

/// `"v5.0.1"` / `"5.0.1"` to `5`.
fn parse_major_version(version: &str) -> Option<u32> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[async_trait]
impl DownloadClient for QBittorrent {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let session = Self::establish(rt, ctx).await?;
        rt.sessions.set(ctx, &session, Some(SESSION_TTL));
        Ok(())
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        let torrents = Self::selected(rt, ctx, limit, selection).await?;
        let now_ms = Utc::now().timestamp_millis() as f64;
        let items: Vec<DownloadClientItem> = torrents.iter().map(|t| map_item(t, now_ms)).collect();
        if selection.is_none() {
            return Ok(DownloadClientJobsAndStatus {
                status: status_of(&torrents),
                items,
                ratio_items: None,
            });
        }
        // The selected window is not the whole client: take the totals from the transfer info.
        let (transfer, running, ratio_items) = tokio::join!(
            async {
                let (_, resp) = Self::request(rt, ctx, "/api/v2/transfer/info", &[], None).await?;
                let value: Value = http::read_json(ctx, resp).await?;
                http::from_value::<TransferInfo>(ctx, "the qBittorrent transfer info", value)
            },
            Self::list(rt, ctx, &[("filter", "running"), ("limit", "1")]),
            Self::ratio_items(rt, ctx, limit),
        );
        let transfer = transfer?;
        Ok(DownloadClientJobsAndStatus {
            status: DownloadClientStatus {
                paused: running?.is_empty(),
                rates: DownloadClientRates {
                    down: transfer.dl_info_speed,
                    up: Some(transfer.up_info_speed),
                },
                types: vec![DownloadClientType::Torrent],
            },
            items,
            ratio_items: Some(ratio_items?),
        })
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::torrents_action(rt, ctx, "pause", "all").await
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::torrents_action(rt, ctx, "resume", "all").await
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::torrents_action(rt, ctx, "pause", &item.id).await
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::torrents_action(rt, ctx, "resume", &item.id).await
    }

    async fn delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> HomarrResult<()> {
        Self::request(
            rt,
            ctx,
            "/api/v2/torrents/delete",
            &[],
            Some(&[
                ("hashes", &item.id),
                ("deleteFiles", if from_disk { "true" } else { "false" }),
            ]),
        )
        .await
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn torrent(value: Value) -> Torrent {
        serde_json::from_value(value).unwrap()
    }

    fn selection() -> DownloadClientSelection {
        DownloadClientSelection {
            sort: "name".into(),
            descending: false,
            category_filter: vec![],
            filter_is_whitelist: false,
            show_completed_torrent: false,
            active_torrent_threshold: 0.0,
        }
    }

    #[test]
    fn maps_states() {
        assert_eq!(
            torrent_state("forcedMetaDL"),
            DownloadClientItemState::Leeching
        );
        assert_eq!(torrent_state("stalledUP"), DownloadClientItemState::Seeding);
        assert_eq!(torrent_state("stoppedDL"), DownloadClientItemState::Paused);
        assert_eq!(torrent_state("stalledDL"), DownloadClientItemState::Stalled);
        assert_eq!(torrent_state("moving"), DownloadClientItemState::Unknown);
    }

    #[test]
    fn maps_eta_and_completion() {
        let downloading = torrent(
            json!({"hash": "a", "progress": 0.4, "eta": 8640000, "dlspeed": 5,
            "state": "downloading"}),
        );
        let item = map_item(&downloading, 0.0);
        assert_eq!(item.time, 0.0);
        assert_eq!(item.down_speed, Some(5.0));
        let finished = torrent(
            json!({"hash": "b", "progress": 1, "completion_on": 100, "state": "uploading"}),
        );
        let item = map_item(&finished, 250_000.0);
        assert_eq!(item.time, -150_000.0);
        assert_eq!(item.down_speed, None);
    }

    #[test]
    fn selection_rules() {
        let mut sel = selection();
        let done = torrent(json!({"hash": "a", "progress": 1, "upspeed": 2048, "category": "tv"}));
        assert!(!accepts(&sel, &done));
        sel.show_completed_torrent = true;
        assert!(accepts(&sel, &done));
        sel.active_torrent_threshold = 3.0;
        assert!(!accepts(&sel, &done));
        sel.active_torrent_threshold = 0.0;
        sel.category_filter = vec!["tv".into()];
        sel.filter_is_whitelist = false;
        assert!(!accepts(&sel, &done));
        sel.filter_is_whitelist = true;
        assert!(accepts(&sel, &done));
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_major_version("v5.0.1"), Some(5));
        assert_eq!(parse_major_version("4.6.2\n"), Some(4));
        assert_eq!(parse_major_version("garbage"), None);
        assert_eq!(remote_sort_field("added"), Some("added_on"));
        assert_eq!(remote_sort_field("time"), None);
    }
}
