use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use reqwest::Method;
use serde_json::json;

use crate::clients::{build_request_url, get_json, request_json_with_auth, simple_get_test};
use crate::http_auth::resolve_declared_http_auth;
use crate::types::{DnsHoleSummary, IntegrationContext};

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    match ctx.kind {
        IntegrationKind::PiHole => {
            if simple_get_test(http, ctx, "/api/info/version")
                .await
                .is_ok()
            {
                return Ok(());
            }
            simple_get_test(http, ctx, "/admin/api.php?summaryRaw").await
        }
        IntegrationKind::AdGuardHome => simple_get_test(http, ctx, "/control/status").await,
        IntegrationKind::TechnitiumDns => {
            simple_get_test(http, ctx, "/api/dashboard/stats/get?type=LastHour").await
        }
        _ => Ok(()),
    }
}

pub async fn get_summary(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<DnsHoleSummary> {
    match ctx.kind {
        IntegrationKind::PiHole => {
            if let Ok(v6) = get_json(http, ctx, "/api/stats/summary").await {
                let queries = v6.get("queries").cloned().unwrap_or_default();
                let gravity = v6.get("gravity").cloned().unwrap_or_default();
                return Ok(DnsHoleSummary {
                    status: "enabled".into(),
                    domains_being_blocked: gravity
                        .get("domains_being_blocked")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    ads_blocked_today: queries.get("blocked").and_then(|v| v.as_u64()).unwrap_or(0),
                    ads_percentage_today: queries
                        .get("percent_blocked")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    dns_queries_today: queries.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
                });
            }
            let auth_q = ctx
                .secret("apiKey")
                .map(|k| format!("/admin/api.php?summaryRaw&auth={k}"))
                .unwrap_or_else(|| "/admin/api.php?summaryRaw".to_string());
            let v5 = get_json(http, ctx, &auth_q).await?;
            Ok(DnsHoleSummary {
                status: v5
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("enabled")
                    .to_string(),
                domains_being_blocked: v5
                    .get("domains_being_blocked")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                ads_blocked_today: v5
                    .get("ads_blocked_today")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                ads_percentage_today: v5
                    .get("ads_percentage_today")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                dns_queries_today: v5
                    .get("dns_queries_today")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            })
        }
        IntegrationKind::AdGuardHome => {
            let status_val = get_json(http, ctx, "/control/status").await?;
            let stats_val = get_json(http, ctx, "/control/stats").await?;
            let queries = stats_val
                .get("num_dns_queries")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let blocked = stats_val
                .get("num_blocked_filtering")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pct = if queries > 0 {
                (blocked as f64 / queries as f64) * 100.0
            } else {
                0.0
            };
            Ok(DnsHoleSummary {
                status: if status_val
                    .get("protection_enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true)
                {
                    "enabled".into()
                } else {
                    "disabled".into()
                },
                domains_being_blocked: 0,
                ads_blocked_today: blocked,
                ads_percentage_today: pct,
                dns_queries_today: queries,
            })
        }
        IntegrationKind::TechnitiumDns => {
            let val = get_json(http, ctx, "/api/dashboard/stats/get?type=LastDay").await?;
            let stats = val
                .get("response")
                .and_then(|r| r.get("stats"))
                .cloned()
                .unwrap_or_default();
            let total = stats
                .get("totalQueries")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let blocked = stats
                .get("totalBlocked")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let pct = if total > 0 {
                (blocked as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            Ok(DnsHoleSummary {
                status: "enabled".into(),
                domains_being_blocked: stats
                    .get("totalBlockListZones")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                ads_blocked_today: blocked,
                ads_percentage_today: pct,
                dns_queries_today: total,
            })
        }
        _ => Err(HomarrError::BadRequest(
            "Unsupported DNS hole integration".into(),
        )),
    }
}

pub async fn set_enabled(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    enabled: bool,
) -> HomarrResult<()> {
    let auth = resolve_declared_http_auth(ctx)?;
    match ctx.kind {
        IntegrationKind::AdGuardHome => {
            let _ = request_json_with_auth(
                http,
                ctx,
                Method::POST,
                "/control/dns_config",
                Some(json!({ "protection_enabled": enabled })),
                &auth,
            )
            .await?;
            Ok(())
        }
        IntegrationKind::PiHole => {
            let url = build_request_url(ctx.base_url(), "/api/dns/blocking", &auth.query)?;
            let mut req = http
                .standard()
                .post(url)
                .json(&json!({ "blocking": enabled }));
            for (k, v) in auth.headers {
                req = req.header(k, v);
            }
            let _ = req.send().await;
            Ok(())
        }
        _ => Ok(()),
    }
}
