use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use serde_json::{json, Value};

use crate::clients::{build_request_url, get_json, simple_get_test};
use crate::http_auth::resolve_declared_http_auth;
use crate::types::{DownloadClientItem, DownloadClientQueue, IntegrationContext};

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    match ctx.kind {
        IntegrationKind::SabNzbd => {
            simple_get_test(http, ctx, "/api?mode=version&output=json").await
        }
        IntegrationKind::NzbGet => {
            let auth = resolve_declared_http_auth(ctx)?;
            let url = build_request_url(ctx.base_url(), "/jsonrpc", &auth.query)?;
            let mut req = http
                .standard()
                .post(url)
                .json(&json!({ "method": "version", "params": [] }));
            for (k, v) in &auth.headers {
                req = req.header(k, v);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("NZBGet request failed: {e}")))?;
            if !resp.status().is_success() {
                return Err(HomarrError::Upstream(format!(
                    "NZBGet HTTP {}",
                    resp.status()
                )));
            }
            Ok(())
        }
        IntegrationKind::QBittorrent => {
            let _ = qbit_login_cookie(http, ctx).await?;
            Ok(())
        }
        IntegrationKind::Transmission => {
            let _ = transmission_rpc(http, ctx, "session-get", json!({})).await?;
            Ok(())
        }
        IntegrationKind::Deluge => {
            let _ = deluge_login(http, ctx).await?;
            Ok(())
        }
        IntegrationKind::Aria2 => {
            let mut params = vec![];
            if let Some(tok) = ctx.secret("apiKey") {
                params.push(json!(format!("token:{tok}")));
            }
            let url = build_request_url(ctx.base_url(), "/jsonrpc", &Default::default())?;
            let resp = http
                .standard()
                .post(url)
                .json(&json!({ "jsonrpc": "2.0", "id": "homarr", "method": "aria2.getVersion", "params": params }))
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("Aria2 request failed: {e}")))?;
            if !resp.status().is_success() {
                return Err(HomarrError::Upstream(format!(
                    "Aria2 HTTP {}",
                    resp.status()
                )));
            }
            Ok(())
        }
        IntegrationKind::Slskd => simple_get_test(http, ctx, "/api/v0/application").await,
        _ => Ok(()),
    }
}

async fn qbit_login_cookie(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<String>> {
    if ctx.secret("apiKey").is_some() {
        simple_get_test(http, ctx, "/api/v2/app/version").await?;
        return Ok(None);
    }
    let user = ctx.require_secret("username")?;
    let pass = ctx.require_secret("password")?;
    let url = build_request_url(ctx.base_url(), "/api/v2/auth/login", &Default::default())?;
    let resp = http
        .standard()
        .post(url)
        .form(&[("username", user), ("password", pass)])
        .send()
        .await
        .map_err(|e| HomarrError::Upstream(format!("qBittorrent login failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(HomarrError::Upstream(format!(
            "qBittorrent login HTTP {}",
            resp.status()
        )));
    }
    let cookie = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|s| s.split(';').next().map(str::to_string));
    Ok(cookie)
}

async fn transmission_rpc(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    method: &str,
    arguments: Value,
) -> HomarrResult<Value> {
    let auth = resolve_declared_http_auth(ctx)?;
    let url = build_request_url(ctx.base_url(), "/transmission/rpc", &auth.query)?;
    let payload = json!({ "method": method, "arguments": arguments });

    let mut session_id: Option<String> = None;
    for _ in 0..2 {
        let mut req = http.standard().post(url.clone()).json(&payload);
        for (k, v) in &auth.headers {
            req = req.header(k, v);
        }
        if let Some(ref sid) = session_id {
            req = req.header("X-Transmission-Session-Id", sid);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Transmission request failed: {e}")))?;
        if resp.status().as_u16() == 409 {
            session_id = resp
                .headers()
                .get("X-Transmission-Session-Id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            continue;
        }
        if !resp.status().is_success() {
            return Err(HomarrError::Upstream(format!(
                "Transmission HTTP {}",
                resp.status()
            )));
        }
        return resp
            .json::<Value>()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Transmission decode error: {e}")));
    }
    Err(HomarrError::Upstream(
        "Transmission session negotiation failed".to_string(),
    ))
}

async fn deluge_login(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<String>> {
    let pass = ctx.require_secret("password")?;
    let url = build_request_url(ctx.base_url(), "/json", &Default::default())?;
    let resp = http
        .standard()
        .post(url)
        .json(&json!({ "method": "auth.login", "params": [pass], "id": 1 }))
        .send()
        .await
        .map_err(|e| HomarrError::Upstream(format!("Deluge login failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(HomarrError::Upstream(format!(
            "Deluge HTTP {}",
            resp.status()
        )));
    }
    let cookie = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|s| s.split(';').next().map(str::to_string));
    Ok(cookie)
}

pub async fn get_queue(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<DownloadClientQueue> {
    match ctx.kind {
        IntegrationKind::SabNzbd => {
            let val = get_json(http, ctx, "/api?mode=queue&output=json").await?;
            let q = val.get("queue").cloned().unwrap_or(Value::Null);
            let slots = q
                .get("slots")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let kb_per_sec = q
                .get("kbpersec")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let items = slots
                .iter()
                .map(|s| {
                    let mb = s
                        .get("mb")
                        .and_then(|v| v.as_str())
                        .and_then(|x| x.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let pct = s
                        .get("percentage")
                        .and_then(|v| v.as_str())
                        .and_then(|x| x.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    DownloadClientItem {
                        id: s
                            .get("nzo_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        name: s
                            .get("filename")
                            .and_then(|v| v.as_str())
                            .unwrap_or("NZB")
                            .to_string(),
                        state: s
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("queued")
                            .to_lowercase(),
                        size_bytes: (mb * 1024.0 * 1024.0) as u64,
                        progress: (pct / 100.0).clamp(0.0, 1.0),
                        download_speed_bps: 0,
                        upload_speed_bps: 0,
                        eta_seconds: None,
                        category: s.get("cat").and_then(|v| v.as_str()).map(str::to_string),
                    }
                })
                .collect();
            Ok(DownloadClientQueue {
                download_speed_bps: (kb_per_sec * 1024.0) as u64,
                upload_speed_bps: 0,
                paused: q.get("paused").and_then(|v| v.as_bool()).unwrap_or(false),
                items,
            })
        }
        IntegrationKind::QBittorrent => {
            let cookie = qbit_login_cookie(http, ctx).await?;
            let url =
                build_request_url(ctx.base_url(), "/api/v2/torrents/info", &Default::default())?;
            let mut req = http.standard().get(url);
            if let Some(c) = cookie {
                req = req.header(reqwest::header::COOKIE, c);
            } else {
                let auth = resolve_declared_http_auth(ctx)?;
                for (k, v) in auth.headers {
                    req = req.header(k, v);
                }
            }
            let val = req
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("qBittorrent torrents failed: {e}")))?
                .json::<Value>()
                .await
                .map_err(|e| HomarrError::Upstream(format!("qBittorrent decode error: {e}")))?;
            let arr = val.as_array().cloned().unwrap_or_default();
            let mut dl_total = 0u64;
            let mut ul_total = 0u64;
            let items = arr
                .iter()
                .map(|t| {
                    let dl = t.get("dlspeed").and_then(|v| v.as_u64()).unwrap_or(0);
                    let ul = t.get("upspeed").and_then(|v| v.as_u64()).unwrap_or(0);
                    dl_total += dl;
                    ul_total += ul;
                    DownloadClientItem {
                        id: t
                            .get("hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        name: t
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Torrent")
                            .to_string(),
                        state: t
                            .get("state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        size_bytes: t.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                        progress: t.get("progress").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        download_speed_bps: dl,
                        upload_speed_bps: ul,
                        eta_seconds: t.get("eta").and_then(|v| v.as_u64()),
                        category: t
                            .get("category")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    }
                })
                .collect();
            Ok(DownloadClientQueue {
                download_speed_bps: dl_total,
                upload_speed_bps: ul_total,
                paused: false,
                items,
            })
        }
        IntegrationKind::Transmission => {
            let val = transmission_rpc(
                http,
                ctx,
                "torrent-get",
                json!({ "fields": ["id", "name", "status", "totalSize", "percentDone", "rateDownload", "rateUpload", "eta"] }),
            )
            .await?;
            let torrents = val
                .get("arguments")
                .and_then(|a| a.get("torrents"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut dl_total = 0u64;
            let mut ul_total = 0u64;
            let items = torrents
                .iter()
                .map(|t| {
                    let dl = t.get("rateDownload").and_then(|v| v.as_u64()).unwrap_or(0);
                    let ul = t.get("rateUpload").and_then(|v| v.as_u64()).unwrap_or(0);
                    dl_total += dl;
                    ul_total += ul;
                    DownloadClientItem {
                        id: t
                            .get("id")
                            .and_then(|v| v.as_i64())
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        name: t
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Torrent")
                            .to_string(),
                        state: match t.get("status").and_then(|v| v.as_i64()).unwrap_or(0) {
                            0 => "paused",
                            4 => "downloading",
                            6 => "seeding",
                            _ => "queued",
                        }
                        .to_string(),
                        size_bytes: t.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0),
                        progress: t.get("percentDone").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        download_speed_bps: dl,
                        upload_speed_bps: ul,
                        eta_seconds: t
                            .get("eta")
                            .and_then(|v| v.as_i64())
                            .filter(|n| *n >= 0)
                            .map(|n| n as u64),
                        category: None,
                    }
                })
                .collect();
            Ok(DownloadClientQueue {
                download_speed_bps: dl_total,
                upload_speed_bps: ul_total,
                paused: false,
                items,
            })
        }
        _ => Ok(DownloadClientQueue {
            download_speed_bps: 0,
            upload_speed_bps: 0,
            paused: false,
            items: Vec::new(),
        }),
    }
}
