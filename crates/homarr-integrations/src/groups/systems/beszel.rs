//! Beszel (`beszel/beszel-integration.ts`): PocketBase-backed hub API with token sessions.
//!
//! The one-second `rt_metrics` SSE live stream (`subscribeRealtimeMetrics`) is not ported: nothing in the Rust
//! workspace consumes it yet.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::http;
use crate::error::{parse_error, request_error, status_error};
use crate::http_auth::ResolvedHttpAuth;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// Re-authenticate this long before the token lapses so no request carries a token that expires in flight.
const SESSION_EXPIRY_LEEWAY_MS: i64 = 60_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeszelSession {
    pub token: String,
    pub user_id: String,
    /// Epoch milliseconds from the token's `exp` claim, when readable.
    pub expires_at: Option<i64>,
}

/// Reads `exp` out of a PocketBase auth token as epoch milliseconds.
pub fn parse_token_expiration(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    let exp = claims.get("exp")?.as_f64().filter(|n| n.is_finite())?;
    Some((exp * 1000.0) as i64)
}

/// Beszel answers requests with an expired token with `200` and an empty list instead of `401`, so sessions
/// have to expire themselves.
pub fn is_session_expired(session: &BeszelSession, now_ms: i64) -> bool {
    session
        .expires_at
        .is_some_and(|at| at - SESSION_EXPIRY_LEEWAY_MS <= now_ms)
}

fn escape_filter_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<BeszelSession> {
    let req = http::request(
        rt,
        ctx,
        Method::POST,
        "/api/collections/users/auth-with-password",
        &[],
    )?
    .json(&json!({
        "identity": ctx.require_secret("username")?,
        "password": ctx.require_secret("password")?,
    }));
    let data = http::send_json(ctx, req, "Beszel auth response").await?;
    let token = http::str_of(&data, "token")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| parse_error(ctx, "Beszel auth response", "missing token"))?
        .to_string();
    let user_id = data
        .pointer("/record/id")
        .and_then(Value::as_str)
        .ok_or_else(|| parse_error(ctx, "Beszel auth response", "missing record id"))?
        .to_string();
    let expires_at = parse_token_expiration(&token);
    let now = Utc::now().timestamp_millis();
    // A token that already lapsed means the clocks disagree by more than its lifetime.
    if expires_at.is_some_and(|at| at <= now) {
        return Err(HomarrError::Upstream(format!(
            "{} issued an already-expired authentication token, check for clock skew",
            ctx.name
        )));
    }
    let session = BeszelSession {
        token,
        user_id,
        expires_at,
    };
    // The stored entry gets the token's own lifetime so the cache cannot outlive the credential.
    let ttl = expires_at.map(|at| Duration::from_millis((at - now).max(1000) as u64));
    rt.sessions.set(ctx, &session, ttl);
    Ok(session)
}

async fn authenticate(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<BeszelSession> {
    if let Some(session) = rt.sessions.get::<BeszelSession>(ctx) {
        if !is_session_expired(&session, Utc::now().timestamp_millis()) {
            return Ok(session);
        }
        rt.sessions.clear(ctx);
    }
    login(rt, ctx).await
}

/// Credentials for custom widget HTTP requests (port of `getHttpAuthenticationAsync`).
pub async fn http_auth(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ResolvedHttpAuth> {
    let token = authenticate(rt, ctx).await?.token;
    Ok(ResolvedHttpAuth {
        redact_values: vec![token.clone()],
        headers: [("Authorization".to_string(), token)].into(),
        ..Default::default()
    })
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let req = http::request(
        rt,
        ctx,
        Method::POST,
        "/api/collections/users/auth-with-password",
        &[],
    )?
    .json(&json!({
        "identity": ctx.require_secret("username")?,
        "password": ctx.require_secret("password")?,
    }));
    let data = http::send_json(ctx, req, "Beszel auth response").await?;
    if http::str_of(&data, "token").is_none_or(str::is_empty) {
        return Err(status_error(ctx, StatusCode::UNAUTHORIZED));
    }
    Ok(())
}

/// Authenticated request; a `401` clears the session and retries once.
async fn fetch(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
    body: Option<Value>,
) -> HomarrResult<Value> {
    let mut session = authenticate(rt, ctx).await?;
    for attempt in 0..2 {
        let mut req = http::request(rt, ctx, method.clone(), path, query)?
            .header("Authorization", &session.token);
        if let Some(body) = &body {
            req = req.json(body);
        }
        let response = req.send().await.map_err(|err| request_error(ctx, &err))?;
        if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
            tracing::warn!(integration = %ctx.name, path, "Beszel 401, clearing session and retrying");
            rt.sessions.clear(ctx);
            session = authenticate(rt, ctx).await?;
            continue;
        }
        if !response.status().is_success() {
            return Err(status_error(ctx, response.status()));
        }
        let text = response
            .text()
            .await
            .map_err(|err| request_error(ctx, &err))?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        return serde_json::from_str(&text).map_err(|err| parse_error(ctx, path, err));
    }
    Err(status_error(ctx, StatusCode::UNAUTHORIZED))
}

fn items(ctx: &IntegrationContext, what: &str, response: Value) -> HomarrResult<Vec<Value>> {
    match response.get("items") {
        Some(Value::Array(items)) => Ok(items.clone()),
        _ => Err(parse_error(ctx, what, "missing items")),
    }
}

async fn list(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    collection: &str,
    query: &[(&str, &str)],
) -> HomarrResult<Vec<Value>> {
    let path = format!("/api/collections/{collection}/records");
    items(
        ctx,
        collection,
        fetch(rt, ctx, Method::GET, &path, query, None).await?,
    )
}

pub async fn systems(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Value>> {
    list(
        rt,
        ctx,
        "systems",
        &[("perPage", "500"), ("sort", "-updated")],
    )
    .await
}

pub async fn system_details(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<Value> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(system_id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/collections/system_details/records/{encoded_id}");
    fetch(rt, ctx, Method::GET, &path, &[], None).await
}

fn system_type_filter(system_id: &str, stats_type: &str) -> String {
    format!(
        "system='{}' && type='{}'",
        escape_filter_value(system_id),
        escape_filter_value(stats_type)
    )
}

pub async fn system_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
    stats_type: &str,
    per_page: u32,
) -> HomarrResult<Vec<Value>> {
    let filter = system_type_filter(system_id, stats_type);
    let per_page = per_page.to_string();
    list(
        rt,
        ctx,
        "system_stats",
        &[
            ("filter", &filter),
            ("sort", "-created"),
            ("perPage", &per_page),
        ],
    )
    .await
}

pub async fn containers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<Vec<Value>> {
    let filter = format!("system='{}'", escape_filter_value(system_id));
    list(
        rt,
        ctx,
        "containers",
        &[
            ("filter", &filter),
            ("sort", "-updated"),
            ("perPage", "500"),
        ],
    )
    .await
}

pub async fn container_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
    stats_type: &str,
    per_page: u32,
) -> HomarrResult<Vec<Value>> {
    let filter = system_type_filter(system_id, stats_type);
    let per_page = per_page.to_string();
    list(
        rt,
        ctx,
        "container_stats",
        &[
            ("filter", &filter),
            ("sort", "-created"),
            ("perPage", &per_page),
        ],
    )
    .await
}

pub async fn smart_devices(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<Vec<Value>> {
    let filter = format!("system='{}'", escape_filter_value(system_id));
    list(rt, ctx, "smart_devices", &[("filter", &filter)]).await
}

pub async fn systemd_services(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<Vec<Value>> {
    let filter = format!("system='{}'", escape_filter_value(system_id));
    list(
        rt,
        ctx,
        "systemd_services",
        &[("filter", &filter), ("sort", "name")],
    )
    .await
}

pub async fn alerts(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: Option<&str>,
) -> HomarrResult<Vec<Value>> {
    let filter = system_id.map(|id| format!("system='{}'", escape_filter_value(id)));
    let mut query = vec![("perPage", "500")];
    if let Some(filter) = &filter {
        query.push(("filter", filter));
    }
    list(rt, ctx, "alerts", &query).await
}

pub async fn alert_history(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: Option<&str>,
    per_page: u32,
) -> HomarrResult<Vec<Value>> {
    let filter = system_id.map(|id| format!("system='{}'", escape_filter_value(id)));
    let per_page = per_page.to_string();
    let mut query = vec![("sort", "-created"), ("perPage", per_page.as_str())];
    if let Some(filter) = &filter {
        query.push(("filter", filter));
    }
    list(rt, ctx, "alerts_history", &query).await
}

/// `input` carries `name`, `value` and optionally `min` (default 0).
pub async fn create_alert(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
    input: &Value,
) -> HomarrResult<Value> {
    let session = authenticate(rt, ctx).await?;
    let body = json!({
        "system": system_id,
        "user": session.user_id,
        "name": input.get("name"),
        "value": input.get("value"),
        "min": input.get("min").cloned().unwrap_or(json!(0)),
        "triggered": false,
    });
    fetch(
        rt,
        ctx,
        Method::POST,
        "/api/collections/alerts/records",
        &[],
        Some(body),
    )
    .await
}

pub async fn update_alert(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    alert_id: &str,
    input: Value,
) -> HomarrResult<Value> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(alert_id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/collections/alerts/records/{encoded_id}");
    fetch(rt, ctx, Method::PATCH, &path, &[], Some(input)).await
}

pub async fn delete_alert(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    alert_id: &str,
) -> HomarrResult<()> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(alert_id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/collections/alerts/records/{encoded_id}");
    fetch(rt, ctx, Method::DELETE, &path, &[], None)
        .await
        .map(|_| ())
}

pub async fn pause_system(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<()> {
    set_system_status(rt, ctx, system_id, "paused").await
}

pub async fn resume_system(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<()> {
    set_system_status(rt, ctx, system_id, "pending").await
}

async fn set_system_status(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
    status: &str,
) -> HomarrResult<()> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(system_id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/collections/systems/records/{encoded_id}");
    fetch(
        rt,
        ctx,
        Method::PATCH,
        &path,
        &[],
        Some(json!({ "status": status })),
    )
    .await
    .map(|_| ())
}

pub async fn delete_system(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    system_id: &str,
) -> HomarrResult<()> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(system_id, percent_encoding::NON_ALPHANUMERIC);
    let path = format!("/api/collections/systems/records/{encoded_id}");
    fetch(rt, ctx, Method::DELETE, &path, &[], None)
        .await
        .map(|_| ())
}

/// Counts systems per status (`up`, `down`, `paused`, `pending`).
pub fn count_by_status(systems: &[Value]) -> [(String, usize); 4] {
    ["up", "down", "paused", "pending"].map(|status| {
        (
            status.to_string(),
            systems
                .iter()
                .filter(|s| http::str_of(s, "status") == Some(status))
                .count(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(payload: &str) -> String {
        format!("header.{}.signature", URL_SAFE_NO_PAD.encode(payload))
    }

    #[test]
    fn reads_exp_claim_as_millis() {
        assert_eq!(
            parse_token_expiration(&token(r#"{"exp":1800000000}"#)),
            Some(1_800_000_000_000)
        );
        // Payload that base64-encodes with '-' and '_' in the url-safe alphabet.
        assert_eq!(
            parse_token_expiration(&token(r#"{"sub":"a>b?c>>","exp":1700000000}"#)),
            Some(1_700_000_000_000)
        );
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert_eq!(parse_token_expiration("nodots"), None);
        assert_eq!(parse_token_expiration("a.!!!.c"), None);
        assert_eq!(parse_token_expiration(&token(r#"{"exp":"soon"}"#)), None);
        assert_eq!(parse_token_expiration(&token(r#"{"sub":"x"}"#)), None);
    }

    #[test]
    fn sessions_expire_with_leeway() {
        let session = |expires_at| BeszelSession {
            token: "t".into(),
            user_id: "u".into(),
            expires_at,
        };
        let now = 1_000_000;
        assert!(!is_session_expired(&session(None), now));
        assert!(!is_session_expired(&session(Some(now + 120_000)), now));
        assert!(is_session_expired(&session(Some(now + 60_000)), now));
        assert!(is_session_expired(&session(Some(now - 1)), now));
    }

    #[test]
    fn escapes_filter_values() {
        assert_eq!(escape_filter_value(r"a'b\c"), r"a\'b\\c");
        assert_eq!(system_type_filter("x", "1m"), "system='x' && type='1m'");
    }

    #[test]
    fn counts_systems_per_status() {
        let systems = vec![
            json!({"status":"up"}),
            json!({"status":"up"}),
            json!({"status":"down"}),
        ];
        let counts = count_by_status(&systems);
        assert_eq!(counts[0], ("up".to_string(), 2));
        assert_eq!(counts[1], ("down".to_string(), 1));
    }
}
