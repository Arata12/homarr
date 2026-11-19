//! Pi-hole v5 and v6 (port of `pi-hole/*`).
//!
//! v5 serves its API at `/admin/api.php` with an `auth` query token; v6 at `/api` with a session id (`sid`)
//! obtained from `POST /api/auth`. The public `/api/info/login` endpoint answers 404 on v5 and 200 on v6 without
//! authentication, which is how the version is detected (falling back to v6 when unreachable).

use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::{json, Value};

use super::http;
use super::DnsHole;
use crate::capabilities::dns_hole::{DnsHoleStatus, DnsHoleSummary};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct PiHole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    V5,
    V6,
}

async fn detect_version(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> Version {
    let Ok(url) = http::url(ctx, "/api/info/login", &[]) else {
        return Version::V6;
    };
    match http::send_raw(ctx, http::get(rt, url)).await {
        Ok(resp) if resp.status().as_u16() == 404 => Version::V5,
        _ => Version::V6,
    }
}

// ---- v5 -----------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct V5Summary {
    status: String,
    domains_being_blocked: f64,
    ads_blocked_today: f64,
    dns_queries_today: f64,
    ads_percentage_today: f64,
}

fn map_v5_summary(summary: V5Summary) -> HomarrResult<DnsHoleSummary> {
    let status = match summary.status.as_str() {
        "enabled" => DnsHoleStatus::Enabled,
        "disabled" => DnsHoleStatus::Disabled,
        other => {
            return Err(HomarrError::Upstream(format!(
                "Pi-hole returned the unknown status '{other}'"
            )))
        }
    };
    Ok(DnsHoleSummary {
        status: Some(status),
        domains_being_blocked: summary.domains_being_blocked as u64,
        ads_blocked_today: summary.ads_blocked_today as u64,
        ads_blocked_today_percentage: summary.ads_percentage_today,
        dns_queries_today: summary.dns_queries_today as u64,
    })
}

async fn v5_call(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<reqwest::Response> {
    let api_key = ctx.require_secret("apiKey")?;
    let url = http::url_ordered(ctx, path, &[("auth", api_key)])?;
    http::send(ctx, http::get(rt, url)).await
}

// ---- v6 -----------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SessionResponse {
    session: SessionInfo,
}

#[derive(Debug, Deserialize)]
struct SessionInfo {
    valid: bool,
    sid: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DnsBlocking {
    blocking: String,
}

#[derive(Debug, Deserialize)]
struct StatsSummary {
    queries: Queries,
    gravity: Gravity,
}

#[derive(Debug, Deserialize)]
struct Queries {
    total: f64,
    blocked: f64,
    percent_blocked: f64,
}

#[derive(Debug, Deserialize)]
struct Gravity {
    domains_being_blocked: f64,
}

/// `failed` / `unknown` (or anything else) mean the status could not be determined.
fn blocking_status(blocking: &str) -> Option<DnsHoleStatus> {
    match blocking {
        "enabled" => Some(DnsHoleStatus::Enabled),
        "disabled" => Some(DnsHoleStatus::Disabled),
        _ => None,
    }
}

fn map_v6_summary(stats: StatsSummary, blocking: &DnsBlocking) -> DnsHoleSummary {
    DnsHoleSummary {
        status: blocking_status(&blocking.blocking),
        domains_being_blocked: stats.gravity.domains_being_blocked as u64,
        ads_blocked_today: stats.queries.blocked as u64,
        ads_blocked_today_percentage: stats.queries.percent_blocked,
        dns_queries_today: stats.queries.total as u64,
    }
}

async fn v6_login(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<String>> {
    let Some(api_key) = ctx.secret("apiKey") else {
        return Ok(None);
    };
    let url = http::url(ctx, "/api/auth", &[])?;
    let value = http::send_json(
        ctx,
        http::post(rt, url).json(&json!({ "password": api_key })),
    )
    .await?;
    let response: SessionResponse = http::from_value(ctx, "the Pi-hole session", value)?;
    if !response.session.valid {
        return Err(HomarrError::Upstream(format!(
            "{} rejected the credentials (HTTP 401){}",
            ctx.name,
            response
                .session
                .message
                .map(|message| format!(": {message}"))
                .unwrap_or_default()
        )));
    }
    Ok(response.session.sid)
}

/// Runs a v6 request with the stored session id, logging in (and storing the sid) when there is none or the
/// stored one answered 401. Reusing the session avoids exhausting Pi-hole's limited API seats.
async fn v6_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> HomarrResult<reqwest::Response> {
    let build = |sid: &Option<String>| -> HomarrResult<reqwest::RequestBuilder> {
        let url = http::url(ctx, path, &[])?;
        let mut req = rt.http.standard().request(method.clone(), url);
        if let Some(sid) = sid {
            req = req.header("sid", sid);
        }
        if let Some(body) = &body {
            req = req.json(body);
        }
        Ok(req)
    };

    if ctx.secret("apiKey").is_none() {
        return http::send(ctx, build(&None)?).await;
    }
    if let Some(stored) = rt.sessions.get::<Option<String>>(ctx) {
        let resp = http::send_raw(ctx, build(&stored)?).await?;
        if resp.status().as_u16() != 401 {
            return crate::error::check_status(ctx, resp);
        }
    }
    let sid = v6_login(rt, ctx).await?;
    rt.sessions.set(ctx, &sid, None);
    http::send(ctx, build(&sid)?).await
}

async fn v6_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Value> {
    let resp = v6_request(rt, ctx, reqwest::Method::GET, path, None).await?;
    http::read_json(ctx, resp).await
}

// ---- trait --------------------------------------------------------------------------------------

#[async_trait]
impl DnsHole for PiHole {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        match detect_version(rt, ctx).await {
            Version::V5 => {
                let value =
                    http::read_json(ctx, v5_call(rt, ctx, "/admin/api.php?status").await?).await?;
                // Pi-hole v5 returned an empty array if the API key is wrong.
                if value.is_object() {
                    Ok(())
                } else {
                    Err(HomarrError::Upstream(format!(
                        "{} rejected the credentials (HTTP 401). Check the API key",
                        ctx.name
                    )))
                }
            }
            Version::V6 => {
                let sid = v6_login(rt, ctx).await?;
                if let Some(sid) = sid {
                    // Free the API seat taken by this test session.
                    let url = http::url(ctx, "/api/auth", &[])?;
                    let req = rt.http.standard().delete(url).header("sid", sid);
                    if let Err(err) = http::send(ctx, req).await {
                        tracing::warn!(error = %err, "Failed to clear Pi-hole test session");
                    }
                }
                rt.sessions.clear(ctx);
                Ok(())
            }
        }
    }

    async fn summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary> {
        match detect_version(rt, ctx).await {
            Version::V5 => {
                let value =
                    http::read_json(ctx, v5_call(rt, ctx, "/admin/api.php?summaryRaw").await?)
                        .await?;
                map_v5_summary(http::from_value(ctx, "the Pi-hole summary", value)?)
            }
            Version::V6 => {
                let stats = v6_json(rt, ctx, "/api/stats/summary").await?;
                let blocking = v6_json(rt, ctx, "/api/dns/blocking").await?;
                Ok(map_v6_summary(
                    http::from_value(ctx, "the Pi-hole stats summary", stats)?,
                    &http::from_value(ctx, "the Pi-hole blocking status", blocking)?,
                ))
            }
        }
    }

    async fn enable(&self, rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()> {
        match detect_version(rt, ctx).await {
            Version::V5 => v5_call(rt, ctx, "/admin/api.php?enable").await.map(drop),
            Version::V6 => v6_request(
                rt,
                ctx,
                reqwest::Method::POST,
                "/api/dns/blocking",
                Some(json!({ "blocking": true })),
            )
            .await
            .map(drop),
        }
    }

    async fn disable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        duration: Option<u64>,
    ) -> HomarrResult<()> {
        let duration = duration.filter(|duration| *duration > 0);
        match detect_version(rt, ctx).await {
            Version::V5 => {
                let path = match duration {
                    Some(duration) => format!("/admin/api.php?disable={duration}"),
                    None => "/admin/api.php?disable".to_string(),
                };
                v5_call(rt, ctx, &path).await.map(drop)
            }
            Version::V6 => v6_request(
                rt,
                ctx,
                reqwest::Method::POST,
                "/api/dns/blocking",
                Some(json!({ "blocking": false, "timer": duration })),
            )
            .await
            .map(drop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_v5_summary() {
        let summary: V5Summary = serde_json::from_value(json!({
            "status": "disabled", "domains_being_blocked": 100, "ads_blocked_today": 5,
            "dns_queries_today": 50, "ads_percentage_today": 10.5
        }))
        .unwrap();
        let out = map_v5_summary(summary).unwrap();
        assert_eq!(out.status, Some(DnsHoleStatus::Disabled));
        assert_eq!(out.domains_being_blocked, 100);
        assert_eq!(out.ads_blocked_today_percentage, 10.5);
    }

    #[test]
    fn rejects_unknown_v5_status() {
        let summary: V5Summary = serde_json::from_value(json!({
            "status": "weird", "domains_being_blocked": 0, "ads_blocked_today": 0,
            "dns_queries_today": 0, "ads_percentage_today": 0
        }))
        .unwrap();
        assert!(map_v5_summary(summary).is_err());
    }

    #[test]
    fn maps_v6_summary_and_unknown_blocking() {
        let stats: StatsSummary = serde_json::from_value(json!({
            "queries": {"total": 200, "blocked": 20, "percent_blocked": 10},
            "gravity": {"domains_being_blocked": 12345}
        }))
        .unwrap();
        let blocking: DnsBlocking =
            serde_json::from_value(json!({"blocking": "failed", "timer": null})).unwrap();
        let out = map_v6_summary(stats, &blocking);
        assert_eq!(out.status, None);
        assert_eq!(out.dns_queries_today, 200);
        assert_eq!(out.ads_blocked_today, 20);
        assert_eq!(out.domains_being_blocked, 12345);
        assert_eq!(blocking_status("enabled"), Some(DnsHoleStatus::Enabled));
    }
}
