use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use reqwest::Method;
use serde_json::{json, Value};

use crate::clients::{build_request_url, get_json, request_json_with_auth, simple_get_test};
use crate::http_auth::resolve_declared_http_auth;
use crate::types::{
    IntegrationContext, MediaRequestItem, MediaRequestStats, MediaServerSession,
    MediaTranscodingSummary,
};

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    match ctx.kind {
        IntegrationKind::Jellyfin | IntegrationKind::Emby => {
            simple_get_test(http, ctx, "/System/Info").await
        }
        IntegrationKind::Plex => simple_get_test(http, ctx, "/").await,
        IntegrationKind::Overseerr | IntegrationKind::Jellyseerr | IntegrationKind::Seerr => {
            simple_get_test(http, ctx, "/api/v1/status").await
        }
        IntegrationKind::Tdarr => simple_get_test(http, ctx, "/api/v2/status").await,
        IntegrationKind::Audiobookshelf => simple_get_test(http, ctx, "/api/libraries").await,
        IntegrationKind::Navidrome => {
            let user = ctx.require_secret("username")?;
            let pass = ctx.require_secret("password")?;
            let path = format!("/rest/ping.view?u={user}&p={pass}&v=1.16.1&c=homarr&f=json");
            simple_get_test(http, ctx, &path).await
        }
        IntegrationKind::Immich => simple_get_test(http, ctx, "/api/server/ping").await,
        _ => Ok(()),
    }
}

pub async fn get_current_sessions(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaServerSession>> {
    match ctx.kind {
        IntegrationKind::Jellyfin | IntegrationKind::Emby => {
            let val = get_json(http, ctx, "/Sessions").await?;
            let arr = val.as_array().cloned().unwrap_or_default();
            Ok(arr
                .iter()
                .filter(|s| s.get("NowPlayingItem").is_some())
                .map(|s| {
                    let np = s.get("NowPlayingItem").cloned().unwrap_or(Value::Null);
                    let ps = s.get("PlayState").cloned().unwrap_or(Value::Null);
                    let pos = ps
                        .get("PositionTicks")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let dur = np
                        .get("RunTimeTicks")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0)
                        .max(1.0);
                    MediaServerSession {
                        id: s
                            .get("Id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        username: s
                            .get("UserName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("User")
                            .to_string(),
                        item_name: np
                            .get("Name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Media")
                            .to_string(),
                        series_name: np
                            .get("SeriesName")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        client_name: s
                            .get("Client")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Client")
                            .to_string(),
                        is_paused: ps
                            .get("IsPaused")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        progress_percent: (pos / dur * 100.0).clamp(0.0, 100.0),
                    }
                })
                .collect())
        }
        IntegrationKind::Plex => {
            let auth = resolve_declared_http_auth(ctx)?;
            let mut url = build_request_url(ctx.base_url(), "/status/sessions", &auth.query)?;
            url.set_query(None);
            let mut req = http
                .standard()
                .get(url)
                .header("Accept", "application/json");
            for (k, v) in auth.headers {
                req = req.header(k, v);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("Plex sessions failed: {e}")))?;
            let val = resp.json::<Value>().await.unwrap_or(Value::Null);
            let metadata = val
                .get("MediaContainer")
                .and_then(|m| m.get("Metadata"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(metadata
                .iter()
                .map(|m| MediaServerSession {
                    id: m
                        .get("ratingKey")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    username: m
                        .get("User")
                        .and_then(|u| u.get("title"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("User")
                        .to_string(),
                    item_name: m
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Media")
                        .to_string(),
                    series_name: m
                        .get("grandparentTitle")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    client_name: m
                        .get("Player")
                        .and_then(|p| p.get("product"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Plex")
                        .to_string(),
                    is_paused: m
                        .get("Player")
                        .and_then(|p| p.get("state"))
                        .and_then(|v| v.as_str())
                        == Some("paused"),
                    progress_percent: 0.0,
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn get_requests(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<(Vec<MediaRequestItem>, MediaRequestStats)> {
    let list_val = get_json(http, ctx, "/api/v1/request?take=20&skip=0").await?;
    let count_val = get_json(http, ctx, "/api/v1/request/count")
        .await
        .unwrap_or(Value::Null);

    let results = list_val
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let items = results
        .iter()
        .map(|r| {
            let status_code = r.get("status").and_then(|v| v.as_i64()).unwrap_or(1);
            let status = match status_code {
                1 => "pending",
                2 => "approved",
                3 => "declined",
                _ => "unknown",
            };
            MediaRequestItem {
                id: r
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                title: r
                    .get("media")
                    .and_then(|m| m.get("title").or_else(|| m.get("originalTitle")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Requested Media")
                    .to_string(),
                media_type: r
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("movie")
                    .to_string(),
                status: status.to_string(),
                requested_by: r
                    .get("requestedBy")
                    .and_then(|u| u.get("displayName").or_else(|| u.get("username")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("User")
                    .to_string(),
                created_at: r
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                poster_url: None,
            }
        })
        .collect();

    let stats = MediaRequestStats {
        total: count_val.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
        pending: count_val
            .get("pending")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        approved: count_val
            .get("approved")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        available: count_val
            .get("available")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        declined: count_val
            .get("declined")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    };
    Ok((items, stats))
}

pub async fn approve_or_decline_request(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    request_id: &str,
    approve: bool,
) -> HomarrResult<()> {
    let auth = resolve_declared_http_auth(ctx)?;
    let action = if approve { "approve" } else { "decline" };
    let path = format!("/api/v1/request/{request_id}/{action}");
    let _ = request_json_with_auth(http, ctx, Method::POST, &path, Some(json!({})), &auth).await?;
    Ok(())
}

pub async fn get_tdarr_summary(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<MediaTranscodingSummary> {
    let val = get_json(http, ctx, "/api/v2/status").await?;
    Ok(MediaTranscodingSummary {
        total_files: val
            .get("totalFileCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        queue_count: val.get("table1Count").and_then(|v| v.as_u64()).unwrap_or(0),
        space_saved_bytes: (val.get("sizeDiff").and_then(|v| v.as_f64()).unwrap_or(0.0) * 1e9)
            as u64,
        active_workers: 0,
    })
}
