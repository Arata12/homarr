//! Speedtest Tracker (`speedtest-tracker-integration.ts`).

use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use homarr_common::HomarrResult;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::{parse_json, q, send, send_raw, to_json, unknown_operation, url};
use crate::error::{check_status, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawResult {
    pub id: i64,
    pub ping: Option<f64>,
    #[serde(default)]
    pub download_bits: Option<i64>,
    #[serde(default)]
    pub upload_bits: Option<i64>,
    pub healthy: Option<bool>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedtestTrackerResult {
    pub id: i64,
    pub ping: Option<f64>,
    pub download_bits: Option<i64>,
    pub upload_bits: Option<i64>,
    pub healthy: Option<bool>,
    /// RFC 3339 UTC instant of the (server local wall time) timestamp.
    pub created_at: String,
}

/// Speedtest Tracker reports `created_at` as local wall time (`2026-05-18 18:30:01`).
pub fn parse_timestamp(timestamp: &str) -> Option<DateTime<Utc>> {
    let normalized = timestamp.replace(' ', "T");
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(parsed.with_timezone(&Utc));
    }
    let naive = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|local| local.with_timezone(&Utc))
}

impl RawResult {
    fn into_result(self, ctx: &IntegrationContext) -> HomarrResult<SpeedtestTrackerResult> {
        let created = parse_timestamp(&self.created_at).ok_or_else(|| {
            crate::error::parse_error(
                ctx,
                "Speedtest Tracker result",
                format!("invalid created_at '{}'", self.created_at),
            )
        })?;
        Ok(SpeedtestTrackerResult {
            id: self.id,
            ping: self.ping,
            download_bits: self.download_bits,
            upload_bits: self.upload_bits,
            healthy: self.healthy,
            created_at: created.to_rfc3339_opts(SecondsFormat::Millis, true),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingStats {
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandwidthStats {
    pub avg: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_bits: Option<f64>,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedtestTrackerStats {
    pub ping: PingStats,
    pub download: BandwidthStats,
    pub upload: BandwidthStats,
    pub total_results: i64,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct Collection {
    data: Vec<RawResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedtestTrackerDashboardData {
    pub latest_result: Option<SpeedtestTrackerResult>,
    pub stats: Option<SpeedtestTrackerStats>,
    pub recent_results: Vec<SpeedtestTrackerResult>,
}

fn headers(ctx: &IntegrationContext) -> HomarrResult<[(&'static str, String); 2]> {
    Ok([
        (
            "Authorization",
            format!("Bearer {}", ctx.require_secret("apiKey")?),
        ),
        ("Accept", "application/json".to_string()),
    ])
}

async fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<reqwest::Response> {
    let mut request = rt.http.standard().get(url(ctx, path, query)?);
    for (name, value) in headers(ctx)? {
        request = request.header(name, value);
    }
    send_raw(ctx, request).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let mut request = rt
        .http
        .standard()
        .get(url(ctx, "/api/v1/results/latest", &[])?);
    for (name, value) in headers(ctx)? {
        request = request.header(name, value);
    }
    send(ctx, request).await?;
    Ok(())
}

async fn body<T: serde::de::DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    response: reqwest::Response,
) -> HomarrResult<T> {
    let text = response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    parse_json(ctx, what, &text)
}

pub async fn latest_result(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<SpeedtestTrackerResult>> {
    let response = get(rt, ctx, "/api/v1/results/latest", &[]).await?;
    // Speedtest Tracker returns 404 when no tests have been run yet (fresh install).
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let envelope: Envelope<RawResult> = body(
        ctx,
        "Speedtest Tracker latest result",
        check_status(ctx, response)?,
    )
    .await?;
    envelope.data.into_result(ctx).map(Some)
}

pub async fn stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<SpeedtestTrackerStats>> {
    let response = check_status(ctx, get(rt, ctx, "/api/v1/stats", &[]).await?)?;
    let envelope: Envelope<SpeedtestTrackerStats> =
        body(ctx, "Speedtest Tracker stats", response).await?;
    Ok(Some(envelope.data))
}

pub async fn recent_results(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    per_page: u32,
) -> HomarrResult<Vec<SpeedtestTrackerResult>> {
    let query = [("result_count", q(per_page)), ("sort", q("-created_at"))];
    let response = check_status(ctx, get(rt, ctx, "/api/v1/results", &query).await?)?;
    let collection: Collection = body(ctx, "Speedtest Tracker results", response).await?;
    collection
        .data
        .into_iter()
        .map(|raw| raw.into_result(ctx))
        .collect()
}

pub async fn dashboard_data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SpeedtestTrackerDashboardData> {
    let (latest_result, stats, recent_results) = tokio::try_join!(
        latest_result(rt, ctx),
        stats(rt, ctx),
        recent_results(rt, ctx, 30)
    )?;
    Ok(SpeedtestTrackerDashboardData {
        latest_result,
        stats,
        recent_results,
    })
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    match operation {
        "getLatestResult" => to_json(&latest_result(rt, ctx).await?),
        "getStats" => to_json(&stats(rt, ctx).await?),
        "getRecentResults" => {
            let per_page = super::util::input_u64(input, "perPage").unwrap_or(10) as u32;
            to_json(&recent_results(rt, ctx, per_page).await?)
        }
        "getDashboardData" => to_json(&dashboard_data(rt, ctx).await?),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Timelike;

    use super::*;

    #[test]
    fn parses_created_at_as_local_wall_time() {
        let parsed = parse_timestamp("2026-05-18 18:30:01").unwrap();
        let local = parsed.with_timezone(&Local);
        assert_eq!((local.hour(), local.minute()), (18, 30));
    }

    #[test]
    fn parses_result_with_nullable_fields() {
        let raw: RawResult = serde_json::from_str(
            r#"{"id":1,"ping":null,"download_bits":null,"healthy":null,"created_at":"2026-05-18 18:30:01"}"#,
        )
        .unwrap();
        assert_eq!(raw.upload_bits, None);
        assert!(parse_timestamp("not a date").is_none());
    }
}
