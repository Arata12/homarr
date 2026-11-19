//! AdGuard Home (port of `adguard-home/*`).

use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::{json, Value};

use super::http;
use super::DnsHole;
use crate::capabilities::dns_hole::{DnsHoleStatus, DnsHoleSummary};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct AdGuardHome;

#[derive(Debug, Deserialize)]
struct Stats {
    time_units: String,
    dns_queries: Vec<f64>,
    blocked_filtering: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct Status {
    protection_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct FilteringStatus {
    filters: Option<Vec<Filter>>,
}

#[derive(Debug, Deserialize)]
struct Filter {
    rules_count: f64,
    enabled: bool,
}

/// Hourly buckets are summed (24h window); daily buckets use the latest day.
fn today(values: &[f64], time_units: &str) -> f64 {
    if time_units == "days" {
        values.last().copied().unwrap_or(0.0)
    } else {
        values.iter().sum()
    }
}

fn map_summary(stats: &Stats, status: &Status, filtering: &FilteringStatus) -> DnsHoleSummary {
    let blocked = today(&stats.blocked_filtering, &stats.time_units);
    let queries = today(&stats.dns_queries, &stats.time_units);
    let filtered_domains: f64 = filtering
        .filters
        .iter()
        .flatten()
        .filter(|filter| filter.enabled)
        .map(|filter| filter.rules_count)
        .sum();
    DnsHoleSummary {
        status: Some(if status.protection_enabled {
            DnsHoleStatus::Enabled
        } else {
            DnsHoleStatus::Disabled
        }),
        domains_being_blocked: filtered_domains as u64,
        ads_blocked_today: blocked as u64,
        ads_blocked_today_percentage: if queries > 0.0 {
            blocked / queries * 100.0
        } else {
            0.0
        },
        dns_queries_today: queries as u64,
    }
}

fn authorization(ctx: &IntegrationContext) -> HomarrResult<String> {
    Ok(http::basic_auth_value(
        ctx.require_secret("username")?,
        ctx.require_secret("password")?,
    ))
}

async fn get_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Value> {
    let url = http::url(ctx, path, &[])?;
    let req = http::get(rt, url).header(reqwest::header::AUTHORIZATION, authorization(ctx)?);
    http::send_json(ctx, req).await
}

async fn set_protection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    body: Value,
) -> HomarrResult<()> {
    let url = http::url(ctx, "/control/protection", &[])?;
    let req = http::post(rt, url)
        .header(reqwest::header::AUTHORIZATION, authorization(ctx)?)
        .json(&body);
    http::send(ctx, req).await.map(drop)
}

#[async_trait]
impl DnsHole for AdGuardHome {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        let status = get_json(rt, ctx, "/control/status").await?;
        if status.is_object() {
            Ok(())
        } else {
            Err(HomarrError::Upstream(format!(
                "Failed to parse the status from {}: expected object data",
                ctx.name
            )))
        }
    }

    async fn summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary> {
        let (stats, status, filtering) = tokio::join!(
            get_json(rt, ctx, "/control/stats"),
            get_json(rt, ctx, "/control/status"),
            get_json(rt, ctx, "/control/filtering/status"),
        );
        let stats: Stats = http::from_value(ctx, "the AdGuard Home stats", stats?)?;
        let status: Status = http::from_value(ctx, "the AdGuard Home status", status?)?;
        let filtering: FilteringStatus =
            http::from_value(ctx, "the AdGuard Home filtering status", filtering?)?;
        Ok(map_summary(&stats, &status, &filtering))
    }

    async fn enable(&self, rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()> {
        set_protection(rt, ctx, json!({ "enabled": true })).await
    }

    async fn disable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        duration: Option<u64>,
    ) -> HomarrResult<()> {
        // `duration` is in seconds, AdGuard Home expects milliseconds (0 = until re-enabled).
        let duration_ms = duration.unwrap_or(0).saturating_mul(1000);
        set_protection(
            rt,
            ctx,
            json!({ "enabled": false, "duration": duration_ms }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(units: &str) -> Stats {
        serde_json::from_value(json!({
            "time_units": units, "dns_queries": [10, 20, 30], "blocked_filtering": [1, 2, 3]
        }))
        .unwrap()
    }

    fn status(enabled: bool) -> Status {
        serde_json::from_value(json!({ "protection_enabled": enabled })).unwrap()
    }

    #[test]
    fn sums_hours_and_takes_last_day() {
        let filtering: FilteringStatus = serde_json::from_value(json!({"filters": [
            {"rules_count": 100, "enabled": true}, {"rules_count": 50, "enabled": false}
        ]}))
        .unwrap();
        let hourly = map_summary(&stats("hours"), &status(true), &filtering);
        assert_eq!(hourly.dns_queries_today, 60);
        assert_eq!(hourly.ads_blocked_today, 6);
        assert_eq!(hourly.ads_blocked_today_percentage, 10.0);
        assert_eq!(hourly.domains_being_blocked, 100);
        assert_eq!(hourly.status, Some(DnsHoleStatus::Enabled));
        let daily = map_summary(&stats("days"), &status(false), &filtering);
        assert_eq!(daily.dns_queries_today, 30);
        assert_eq!(daily.ads_blocked_today, 3);
        assert_eq!(daily.status, Some(DnsHoleStatus::Disabled));
    }

    #[test]
    fn null_filters_and_zero_queries() {
        let filtering: FilteringStatus = serde_json::from_value(json!({"filters": null})).unwrap();
        let empty: Stats = serde_json::from_value(json!({
            "time_units": "hours", "dns_queries": [], "blocked_filtering": []
        }))
        .unwrap();
        let out = map_summary(&empty, &status(true), &filtering);
        assert_eq!(out.domains_being_blocked, 0);
        assert_eq!(out.ads_blocked_today_percentage, 0.0);
    }
}
