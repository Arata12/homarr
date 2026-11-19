//! Technitium DNS Server (port of `technitium/*`).
//!
//! All tested versions (v11 to v15) share the API paths; only the auth differs: v15 accepts
//! `Authorization: Bearer <token>`, older versions need `?token=`. The server always answers HTTP 200 and signals
//! `ok` / `invalid-token` / `error` in the body. The session (token + detected version) lives in `rt.sessions`.

use std::future::Future;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http;
use super::DnsHole;
use crate::capabilities::dns_hole::{DnsHoleStatus, DnsHoleSummary};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct Technitium;

const LOGIN: &str = "/api/user/login";
const LEGACY_LOGIN: &str = "/api/login";
const LOGOUT: &str = "/api/user/logout";
const STATS: &str = "/api/dashboard/stats/get";
const SETTINGS_GET: &str = "/api/settings/get";
const SETTINGS_SET: &str = "/api/settings/set";
const TEMPORARY_DISABLE: &str = "/api/settings/temporaryDisableBlocking";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Version {
    V15,
    Legacy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    /// Empty for API key users (the key itself is the token).
    token: String,
    version: Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    InvalidToken,
    Error,
}

enum Fail {
    /// The server reported the token as expired.
    Expired,
    Other(HomarrError),
}

impl From<HomarrError> for Fail {
    fn from(err: HomarrError) -> Self {
        Self::Other(err)
    }
}

fn parse_version(info_version: Option<&str>) -> Version {
    let major = info_version
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        .unwrap_or(0);
    if major >= 15 {
        Version::V15
    } else {
        Version::Legacy
    }
}

fn body_status(data: &Value) -> Status {
    match data.get("status").and_then(Value::as_str) {
        Some("ok") => Status::Ok,
        Some("invalid-token") => Status::InvalidToken,
        _ => Status::Error,
    }
}

fn api_key(ctx: &IntegrationContext) -> Option<&str> {
    ctx.secret("apiKey").filter(|key| !key.is_empty())
}

#[derive(Debug, Deserialize)]
struct Stats {
    #[serde(rename = "totalQueries")]
    total_queries: f64,
    #[serde(rename = "totalBlocked")]
    total_blocked: f64,
    #[serde(rename = "blockedZones")]
    blocked_zones: f64,
    #[serde(rename = "blockListZones")]
    block_list_zones: f64,
}

#[derive(Debug, Deserialize)]
struct StatsResponse {
    response: StatsInner,
}

#[derive(Debug, Deserialize)]
struct StatsInner {
    stats: Stats,
}

#[derive(Debug, Deserialize)]
struct SettingsResponse {
    response: Settings,
}

#[derive(Debug, Deserialize)]
struct Settings {
    #[serde(rename = "enableBlocking")]
    enable_blocking: bool,
    #[serde(rename = "temporaryDisableBlockingTill", default)]
    temporary_disable_blocking_till: Option<String>,
}

fn blocking_status(settings: &Settings, now: DateTime<Utc>) -> DnsHoleStatus {
    if !settings.enable_blocking {
        return DnsHoleStatus::Disabled;
    }
    let disabled_until = settings
        .temporary_disable_blocking_till
        .as_deref()
        .and_then(|till| DateTime::parse_from_rfc3339(till).ok());
    match disabled_until {
        Some(till) if till.with_timezone(&Utc) > now => DnsHoleStatus::Disabled,
        _ => DnsHoleStatus::Enabled,
    }
}

fn map_summary(stats: &Stats, status: Option<DnsHoleStatus>) -> DnsHoleSummary {
    DnsHoleSummary {
        status,
        dns_queries_today: stats.total_queries as u64,
        ads_blocked_today: stats.total_blocked as u64,
        ads_blocked_today_percentage: if stats.total_queries > 0.0 {
            stats.total_blocked / stats.total_queries * 100.0
        } else {
            0.0
        },
        domains_being_blocked: (stats.blocked_zones + stats.block_list_zones) as u64,
    }
}

/// GET with the token as bearer header (v15) or `token` query parameter (legacy).
async fn request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    params: &[(&str, &str)],
    token: Option<&str>,
    version: Version,
) -> HomarrResult<Value> {
    let mut query: Vec<(&str, &str)> = params.to_vec();
    if let (Some(token), Version::Legacy) = (token, version) {
        query.push(("token", token));
    }
    let url = http::url_ordered(ctx, path, &query)?;
    let mut req = http::get(rt, url);
    if let (Some(token), Version::V15) = (token, version) {
        req = req.bearer_auth(token);
    }
    http::send_json(ctx, req).await
}

/// Parses the body and maps `invalid-token` to [`Fail::Expired`].
fn authenticated(data: Value) -> Result<(Status, Value), Fail> {
    match body_status(&data) {
        Status::InvalidToken => Err(Fail::Expired),
        status => Ok((status, data)),
    }
}

/// Bearer on the stats endpoint: `ok` / `invalid-token` mean v15, `error` means legacy `?token=` auth.
async fn detect_version(rt: &IntegrationRuntime, ctx: &IntegrationContext, key: &str) -> Version {
    match request(
        rt,
        ctx,
        STATS,
        &[("type", "LastHour")],
        Some(key),
        Version::V15,
    )
    .await
    {
        Ok(data) if body_status(&data) == Status::Error => Version::Legacy,
        _ => Version::V15,
    }
}

async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<StoredSession> {
    let user = ctx.require_secret("username")?;
    let pass = ctx.require_secret("password")?;
    let query = [("user", user), ("pass", pass), ("includeInfo", "true")];
    let mut url = http::url_ordered(ctx, LOGIN, &query)?;
    let mut resp = http::send_raw(ctx, http::get(rt, url)).await?;
    if resp.status().as_u16() == 404 {
        // Very old servers only know /api/login.
        url = http::url_ordered(ctx, LEGACY_LOGIN, &query)?;
        resp = http::send_raw(ctx, http::get(rt, url)).await?;
    }
    let data = http::read_json(ctx, crate::error::check_status(ctx, resp)?).await?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .filter(|_| body_status(&data) == Status::Ok);
    let Some(token) = token else {
        return Err(HomarrError::Upstream(format!(
            "{} rejected the credentials (HTTP 401). Check the username and password",
            ctx.name
        )));
    };
    let version = parse_version(data.pointer("/info/version").and_then(Value::as_str));
    Ok(StoredSession {
        token: token.to_string(),
        version,
    })
}

/// The token and auth version to use, logging in (or probing the version for API keys) when the cache is cold.
async fn acquire(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<(String, Version)> {
    if let Some(key) = api_key(ctx) {
        let version = match rt.sessions.get::<StoredSession>(ctx) {
            Some(cached) => cached.version,
            None => {
                let version = detect_version(rt, ctx, key).await;
                rt.sessions.set(
                    ctx,
                    &StoredSession {
                        token: String::new(),
                        version,
                    },
                    None,
                );
                version
            }
        };
        return Ok((key.to_string(), version));
    }
    if let Some(stored) = rt.sessions.get::<StoredSession>(ctx) {
        return Ok((stored.token, stored.version));
    }
    let session = login(rt, ctx).await?;
    rt.sessions.set(ctx, &session, None);
    Ok((session.token, session.version))
}

/// Runs the action; on an expired token (credentials auth only) logs in again and retries once.
async fn with_retry<T, F, Fut>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    action: F,
) -> HomarrResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, Fail>>,
{
    let expired = || {
        HomarrError::Upstream(format!(
            "{} rejected the token (HTTP 401). Check the credentials",
            ctx.name
        ))
    };
    match action().await {
        Ok(value) => Ok(value),
        Err(Fail::Other(err)) => Err(err),
        Err(Fail::Expired) if api_key(ctx).is_some() => Err(expired()),
        Err(Fail::Expired) => {
            rt.sessions.clear(ctx);
            let session = login(rt, ctx).await?;
            rt.sessions.set(ctx, &session, None);
            match action().await {
                Ok(value) => Ok(value),
                Err(Fail::Other(err)) => Err(err),
                Err(Fail::Expired) => Err(expired()),
            }
        }
    }
}

fn expect_ok(ctx: &IntegrationContext, data: Value) -> Result<(), Fail> {
    let (status, _) = authenticated(data)?;
    if status == Status::Ok {
        Ok(())
    } else {
        Err(Fail::Other(HomarrError::Upstream(format!(
            "{} reported a server error (HTTP 500)",
            ctx.name
        ))))
    }
}

async fn summary_once(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> Result<DnsHoleSummary, Fail> {
    let (token, version) = acquire(rt, ctx).await?;
    // Sequential: stats is required, settings only when stats succeeded.
    let data = request(
        rt,
        ctx,
        STATS,
        &[("type", "LastHour")],
        Some(&token),
        version,
    )
    .await?;
    let (status, data) = authenticated(data)?;
    if status != Status::Ok {
        return Err(Fail::Other(HomarrError::Upstream(format!(
            "{} reported a server error (HTTP 500)",
            ctx.name
        ))));
    }
    let stats: StatsResponse = http::from_value(ctx, "the Technitium stats", data)?;

    // Blocking status is optional: restricted users or unexpected shapes continue without it.
    let blocking = match request(rt, ctx, SETTINGS_GET, &[], Some(&token), version).await {
        Ok(data) => {
            let (status, data) = authenticated(data)?;
            match (status, serde_json::from_value::<SettingsResponse>(data)) {
                (Status::Ok, Ok(settings)) => Some(blocking_status(&settings.response, Utc::now())),
                _ => {
                    tracing::warn!(integration = %ctx.name, "Could not fetch Technitium blocking status, continuing without it");
                    None
                }
            }
        }
        Err(_) => None,
    };
    Ok(map_summary(&stats.response.stats, blocking))
}

#[async_trait]
impl DnsHole for Technitium {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let unauthorized = || {
            HomarrError::Upstream(format!(
                "{} rejected the credentials (HTTP 401). Check the credentials",
                ctx.name
            ))
        };
        let (token, version, session) = if let Some(key) = api_key(ctx) {
            let version = match rt.sessions.get::<StoredSession>(ctx) {
                Some(cached) => cached.version,
                None => detect_version(rt, ctx, key).await,
            };
            (key.to_string(), version, false)
        } else {
            let session = login(rt, ctx).await?;
            (session.token, session.version, true)
        };
        let data = request(
            rt,
            ctx,
            STATS,
            &[("type", "LastHour")],
            Some(&token),
            version,
        )
        .await?;
        if body_status(&data) != Status::Ok {
            return Err(unauthorized());
        }
        // Free the server-side session slot; API keys are permanent and have none.
        if session {
            if let Err(err) = request(rt, ctx, LOGOUT, &[], Some(&token), version).await {
                tracing::warn!(integration = %ctx.name, error = %err, "Failed to log out Technitium test session");
            }
        }
        rt.sessions.clear(ctx);
        Ok(())
    }

    async fn summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary> {
        with_retry(rt, ctx, || summary_once(rt, ctx)).await
    }

    async fn enable(&self, rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()> {
        with_retry(rt, ctx, || async {
            let (token, version) = acquire(rt, ctx).await?;
            let data = request(
                rt,
                ctx,
                SETTINGS_SET,
                &[("enableBlocking", "true")],
                Some(&token),
                version,
            )
            .await?;
            expect_ok(ctx, data)
        })
        .await
    }

    async fn disable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        duration: Option<u64>,
    ) -> HomarrResult<()> {
        with_retry(rt, ctx, || async {
            let (token, version) = acquire(rt, ctx).await?;
            let data = match duration.filter(|duration| *duration > 0) {
                // Technitium's timed disable takes minutes; seconds are rounded up (1-59s becomes 1 min).
                Some(seconds) => {
                    let minutes = seconds.div_ceil(60).to_string();
                    request(
                        rt,
                        ctx,
                        TEMPORARY_DISABLE,
                        &[("minutes", &minutes)],
                        Some(&token),
                        version,
                    )
                    .await?
                }
                None => {
                    request(
                        rt,
                        ctx,
                        SETTINGS_SET,
                        &[("enableBlocking", "false")],
                        Some(&token),
                        version,
                    )
                    .await?
                }
            };
            expect_ok(ctx, data)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_auth_version_from_server_version() {
        assert_eq!(parse_version(Some("15.0")), Version::V15);
        assert_eq!(parse_version(Some("13.6.1")), Version::Legacy);
        assert_eq!(parse_version(None), Version::Legacy);
    }

    #[test]
    fn reads_body_status() {
        assert_eq!(body_status(&json!({"status": "ok"})), Status::Ok);
        assert_eq!(
            body_status(&json!({"status": "invalid-token"})),
            Status::InvalidToken
        );
        assert_eq!(body_status(&json!({"status": "error"})), Status::Error);
        assert_eq!(body_status(&json!({})), Status::Error);
        assert!(matches!(
            authenticated(json!({"status": "invalid-token"})),
            Err(Fail::Expired)
        ));
    }

    #[test]
    fn computes_blocking_status() {
        let now = DateTime::parse_from_rfc3339("2025-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let settings = |enabled: bool, till: Option<&str>| Settings {
            enable_blocking: enabled,
            temporary_disable_blocking_till: till.map(str::to_string),
        };
        assert_eq!(
            blocking_status(&settings(false, None), now),
            DnsHoleStatus::Disabled
        );
        assert_eq!(
            blocking_status(&settings(true, None), now),
            DnsHoleStatus::Enabled
        );
        assert_eq!(
            blocking_status(&settings(true, Some("2025-01-01T12:05:00Z")), now),
            DnsHoleStatus::Disabled
        );
        assert_eq!(
            blocking_status(&settings(true, Some("2025-01-01T11:00:00Z")), now),
            DnsHoleStatus::Enabled
        );
    }

    #[test]
    fn maps_summary() {
        let stats: StatsResponse =
            serde_json::from_value(json!({"status": "ok", "response": {"stats": {
                "totalQueries": 200, "totalBlocked": 50, "blockedZones": 3, "blockListZones": 7
            }}}))
            .unwrap();
        let out = map_summary(&stats.response.stats, None);
        assert_eq!(out.ads_blocked_today_percentage, 25.0);
        assert_eq!(out.domains_being_blocked, 10);
        assert_eq!(out.status, None);
    }
}
