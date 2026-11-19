//! UniFi Network controller (port of `unifi-controller/*`; the TS code used `node-unifi`).
//!
//! node-unifi always speaks HTTPS to `host:port`, trying 443 (UniFi OS consoles) and 8443 (self-hosted
//! controllers) when the URL has no port. UniFi OS logs in at `/api/auth/login` and proxies the network API
//! below `/proxy/network`; classic controllers use `/api/login` and no prefix. The login cookie and CSRF token
//! are kept in `rt.sessions` for 5 minutes.

use std::time::Duration;

use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use super::http;
use crate::capabilities::network_controller::{
    EnabledStatus, NetworkControllerSummary, UsersSummary, VpnUsersSummary, WwwSummary,
};
use crate::error::{check_status, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const SESSION_TTL: Duration = Duration::from_secs(300);
const FALLBACK_PORTS: [u16; 2] = [443, 8443];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    origin: String,
    cookie: String,
    csrf_token: Option<String>,
    unifios: bool,
}

#[derive(Debug, Deserialize)]
struct Site {
    name: String,
    #[serde(default)]
    health: Vec<Health>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Health {
    subsystem: String,
    status: String,
    latency: f64,
    speedtest_ping: f64,
    uptime: f64,
    num_user: f64,
    num_guest: f64,
    remote_user_num_active: f64,
}

#[derive(Debug, Deserialize)]
struct RemoteVpnSession {
    up: bool,
}

fn subsystem<'a>(site: &'a Site, name: &str) -> HomarrResult<&'a Health> {
    site.health
        .iter()
        .find(|health| health.subsystem == name)
        .ok_or_else(|| HomarrError::Upstream(format!("UniFi subsystem {name} not found")))
}

fn enabled(value: bool) -> EnabledStatus {
    if value {
        EnabledStatus::Enabled
    } else {
        EnabledStatus::Disabled
    }
}

/// Enabled when every site reports `ok` for the subsystem.
fn status_over_sites(sites: &[Site], name: &str) -> HomarrResult<EnabledStatus> {
    let mut all_ok = true;
    for site in sites {
        all_ok &= subsystem(site, name)?.status == "ok";
    }
    Ok(enabled(all_ok))
}

enum Strategy {
    Sum,
    Max,
}

fn numeric_over_sites(
    sites: &[Site],
    name: &str,
    select: impl Fn(&Health) -> f64,
    strategy: Strategy,
) -> HomarrResult<f64> {
    let mut values = Vec::with_capacity(sites.len());
    for site in sites {
        values.push(select(subsystem(site, name)?));
    }
    Ok(match strategy {
        Strategy::Sum => values.iter().sum(),
        Strategy::Max => values.into_iter().fold(0.0, f64::max),
    })
}

/// `vpn_users` is `None` when a site's remote session count was unavailable: a partial total would be wrong.
fn summarize(sites: &[Site], vpn_users: Option<u64>) -> HomarrResult<NetworkControllerSummary> {
    let wan_status = status_over_sites(sites, "wan")?;
    Ok(NetworkControllerSummary {
        wan_status,
        www: WwwSummary {
            status: wan_status,
            latency: numeric_over_sites(sites, "www", |h| h.latency, Strategy::Max)?,
            ping: numeric_over_sites(sites, "www", |h| h.speedtest_ping, Strategy::Max)?,
            uptime: numeric_over_sites(sites, "www", |h| h.uptime, Strategy::Max)?,
        },
        wifi: UsersSummary {
            status: status_over_sites(sites, "wlan")?,
            users: numeric_over_sites(sites, "wlan", |h| h.num_user, Strategy::Sum)? as u64,
            guests: numeric_over_sites(sites, "wlan", |h| h.num_guest, Strategy::Sum)? as u64,
        },
        lan: UsersSummary {
            status: status_over_sites(sites, "lan")?,
            users: numeric_over_sites(sites, "lan", |h| h.num_user, Strategy::Sum)? as u64,
            guests: numeric_over_sites(sites, "lan", |h| h.num_guest, Strategy::Sum)? as u64,
        },
        vpn: VpnUsersSummary {
            status: status_over_sites(sites, "vpn")?,
            users: vpn_users,
        },
    })
}

fn candidate_origins(ctx: &IntegrationContext) -> HomarrResult<Vec<String>> {
    let url = Url::parse(&ctx.url).map_err(|err| {
        HomarrError::BadRequest(format!("Invalid integration URL '{}': {err}", ctx.url))
    })?;
    let host = url.host_str().ok_or_else(|| {
        HomarrError::BadRequest(format!("Integration URL '{}' has no host", ctx.url))
    })?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let ports: Vec<u16> = match url.port() {
        Some(port) => vec![port],
        None => FALLBACK_PORTS.to_vec(),
    };
    Ok(ports
        .into_iter()
        .map(|port| format!("https://{host}:{port}"))
        .collect())
}

fn json_post(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    url: &str,
) -> HomarrResult<reqwest::RequestBuilder> {
    Ok(rt
        .http
        .standard()
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&json!({
            "username": ctx.require_secret("username")?,
            "password": ctx.require_secret("password")?,
        })))
}

/// Logs in at one origin. `Ok(None)` means the origin was unreachable (try the next port).
async fn login_at(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    origin: &str,
    is_last: bool,
) -> HomarrResult<Option<Session>> {
    let mut unifios = true;
    let mut resp = match json_post(rt, ctx, &format!("{origin}/api/auth/login"))?
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) if !is_last && (err.is_connect() || err.is_timeout()) => return Ok(None),
        Err(err) => return Err(request_error(ctx, &err)),
    };
    if resp.status().as_u16() == 404 {
        unifios = false;
        resp = json_post(rt, ctx, &format!("{origin}/api/login"))?
            .send()
            .await
            .map_err(|err| request_error(ctx, &err))?;
    }
    let resp = check_status(ctx, resp)?;
    let cookie = http::cookie_header(&resp).ok_or_else(|| {
        HomarrError::Upstream(format!("{} did not return a session cookie", ctx.name))
    })?;
    let csrf_token = resp
        .headers()
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    Ok(Some(Session {
        origin: origin.to_string(),
        cookie,
        csrf_token,
        unifios,
    }))
}

async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Session> {
    let origins = candidate_origins(ctx)?;
    let last = origins.len() - 1;
    for (index, origin) in origins.iter().enumerate() {
        if let Some(session) = login_at(rt, ctx, origin, index == last).await? {
            return Ok(session);
        }
    }
    Err(HomarrError::Upstream(format!(
        "Unable to connect to the UniFi controller {}",
        ctx.name
    )))
}

async fn session(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Session> {
    if let Some(session) = rt.sessions.get::<Session>(ctx) {
        return Ok(session);
    }
    let session = login(rt, ctx).await?;
    rt.sessions.set(ctx, &session, Some(SESSION_TTL));
    Ok(session)
}

/// Network API request (`path` without the UniFi OS prefix). Returns the HTTP status and the `data` member.
/// A 401 drops the cached session and retries once.
async fn api(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<(u16, Value)> {
    for attempt in 0..2 {
        let session = session(rt, ctx).await?;
        let prefix = if session.unifios {
            "/proxy/network"
        } else {
            ""
        };
        let mut req = rt
            .http
            .standard()
            .request(Method::GET, format!("{}{prefix}{path}", session.origin))
            .header(reqwest::header::COOKIE, &session.cookie);
        if let Some(csrf) = &session.csrf_token {
            req = req.header("X-CSRF-Token", csrf);
        }
        let resp = http::send_raw(ctx, req).await?;
        let status = resp.status().as_u16();
        if status == 401 && attempt == 0 {
            rt.sessions.clear(ctx);
            continue;
        }
        if status == 404 {
            return Ok((status, Value::Null));
        }
        let mut body = http::read_json(ctx, check_status(ctx, resp)?).await?;
        return Ok((
            status,
            body.get_mut("data").map(Value::take).unwrap_or(Value::Null),
        ));
    }
    Err(HomarrError::Upstream(format!(
        "{} rejected the credentials (HTTP 401). Check the username and password",
        ctx.name
    )))
}

async fn sites_stats(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Vec<Site>> {
    let (status, data) = api(rt, ctx, "/api/stat/sites").await?;
    if status == 404 {
        return Err(crate::error::status_error(
            ctx,
            reqwest::StatusCode::NOT_FOUND,
        ));
    }
    http::from_value(ctx, "the UniFi site stats", data)
}

/// Number of up remote VPN sessions of a site; `None` when unavailable. Older controllers without the endpoint
/// fall back to the health subsystem counter.
async fn vpn_users_of_site(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    site: &Site,
) -> HomarrResult<Option<u64>> {
    let encoded = url::form_urlencoded::byte_serialize(site.name.as_bytes()).collect::<String>();
    let path = format!("/api/s/{encoded}/stat/remoteuservpn");
    match api(rt, ctx, &path).await {
        Ok((404, _)) => numeric_over_sites(
            std::slice::from_ref(site),
            "vpn",
            |h| h.remote_user_num_active,
            Strategy::Sum,
        )
        .map(|count| Some(count as u64)),
        Ok((_, data)) => match serde_json::from_value::<Vec<RemoteVpnSession>>(data) {
            Ok(sessions) => Ok(Some(
                sessions.iter().filter(|session| session.up).count() as u64
            )),
            Err(err) => {
                tracing::warn!(integration = %ctx.name, site = %site.name, error = %err, "UniFi remote VPN sessions have an invalid shape");
                Ok(None)
            }
        },
        Err(err) => {
            tracing::warn!(integration = %ctx.name, site = %site.name, error = %err, "UniFi remote VPN session count unavailable");
            Ok(None)
        }
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    rt.sessions.clear(ctx);
    sites_stats(rt, ctx).await.map(drop)
}

pub async fn network_summary(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<NetworkControllerSummary> {
    let sites = sites_stats(rt, ctx).await?;
    let mut counts = Vec::with_capacity(sites.len());
    for site in &sites {
        counts.push(vpn_users_of_site(rt, ctx, site).await?);
    }
    let vpn_users = counts
        .into_iter()
        .try_fold(0u64, |total, count| count.map(|count| total + count));
    summarize(&sites, vpn_users)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(name: &str, wan: &str, www_latency: f64, wlan_users: f64, vpn_remote: f64) -> Site {
        serde_json::from_value(json!({"name": name, "health": [
            {"subsystem": "wan", "status": wan},
            {"subsystem": "www", "status": "ok", "latency": www_latency, "speedtest_ping": 5, "uptime": 1000},
            {"subsystem": "wlan", "status": "ok", "num_user": wlan_users, "num_guest": 1},
            {"subsystem": "lan", "status": "unknown", "num_user": 2, "num_guest": 0},
            {"subsystem": "vpn", "status": "ok", "remote_user_num_active": vpn_remote}
        ]}))
        .unwrap()
    }

    #[test]
    fn aggregates_over_sites() {
        let sites = [
            site("a", "ok", 10.0, 3.0, 1.0),
            site("b", "error", 30.0, 4.0, 2.0),
        ];
        let out = summarize(&sites, Some(3)).unwrap();
        assert_eq!(out.wan_status, EnabledStatus::Disabled);
        assert_eq!(out.www.status, EnabledStatus::Disabled);
        assert_eq!(out.www.latency, 30.0);
        assert_eq!(out.www.ping, 5.0);
        assert_eq!(out.wifi.users, 7);
        assert_eq!(out.wifi.guests, 2);
        assert_eq!(out.wifi.status, EnabledStatus::Enabled);
        assert_eq!(out.lan.status, EnabledStatus::Disabled);
        assert_eq!(out.lan.users, 4);
        assert_eq!(out.vpn.users, Some(3));
    }

    #[test]
    fn missing_subsystem_is_an_error_and_unknown_vpn_count_is_kept_unknown() {
        let broken: Site = serde_json::from_value(json!({"name": "x", "health": []})).unwrap();
        assert!(summarize(&[broken], None).is_err());
        let out = summarize(&[site("a", "ok", 1.0, 1.0, 0.0)], None).unwrap();
        assert_eq!(out.vpn.users, None);
        assert_eq!(out.wan_status, EnabledStatus::Enabled);
    }

    #[test]
    fn builds_origins_with_port_fallbacks() {
        let mut ctx = IntegrationContext {
            id: "i".into(),
            name: "Unifi".into(),
            url: "http://unifi.local".into(),
            external_url: None,
            kind: homarr_definitions::IntegrationKind::UnifiController,
            secrets: Default::default(),
        };
        assert_eq!(
            candidate_origins(&ctx).unwrap(),
            vec!["https://unifi.local:443", "https://unifi.local:8443"]
        );
        ctx.url = "https://10.0.0.1:8443/".into();
        assert_eq!(
            candidate_origins(&ctx).unwrap(),
            vec!["https://10.0.0.1:8443"]
        );
    }
}
