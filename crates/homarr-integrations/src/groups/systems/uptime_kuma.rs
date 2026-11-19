//! Uptime Kuma (`uptime-kuma/*`): public status page + heartbeat endpoints.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use homarr_common::HomarrResult;
use percent_encoding::utf8_percent_encode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, as_f64, str_of};
use crate::error::parse_error;
use crate::http_auth::URI_COMPONENT;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const UPTIME_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const MINIMUM_STALE_AGE_MS: i64 = 10 * 60 * 1000;
const MISSED_CHECKS_UNTIL_STALE: i64 = 5;
pub const MAX_HEARTBEATS: usize = 50;
const INCIDENT_STYLES: [&str; 6] = ["info", "warning", "danger", "primary", "light", "dark"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitorStatus {
    Up,
    Down,
    Pending,
    Maintenance,
    Stale,
    Unknown,
}

pub fn monitor_status(code: i64) -> MonitorStatus {
    match code {
        0 => MonitorStatus::Down,
        1 => MonitorStatus::Up,
        2 => MonitorStatus::Pending,
        3 => MonitorStatus::Maintenance,
        _ => MonitorStatus::Unknown,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub status: MonitorStatus,
    /// Epoch milliseconds.
    pub time: i64,
    pub ping: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: String,
    pub color: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub monitor_type: Option<String>,
    pub url: Option<String>,
    pub status: MonitorStatus,
    pub uptime24h: Option<f64>,
    pub ping: Option<f64>,
    pub last_check_at: Option<i64>,
    pub status_since: Option<i64>,
    pub certificate_days_remaining: Option<f64>,
    pub tags: Vec<Tag>,
    pub heartbeats: Vec<Heartbeat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub monitors: Vec<Monitor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: i64,
    pub title: String,
    pub content: Option<String>,
    pub style: String,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Maintenance {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPage {
    pub title: String,
    pub url: String,
    pub groups: Vec<Group>,
    pub incidents: Vec<Incident>,
    pub maintenances: Vec<Maintenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverallStatus {
    Operational,
    Degraded,
    Outage,
    Maintenance,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total: usize,
    pub counts: HashMap<MonitorStatus, usize>,
    pub average_uptime24h: Option<f64>,
    pub overall_status: OverallStatus,
}

fn slug(ctx: &IntegrationContext) -> String {
    ctx.secret("slug")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "default".to_string())
}

/// `encodeURIComponent` semantics for a path segment (space is `%20`, never `+`).
fn encode(slug: &str) -> String {
    utf8_percent_encode(slug, URI_COMPONENT).to_string()
}

fn status_page_path(slug: &str) -> String {
    format!("/api/status-page/{}", encode(slug))
}

fn heartbeat_path(slug: &str) -> String {
    format!("/api/status-page/heartbeat/{}", encode(slug))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let slug = slug(ctx);
    let page = http::send_json(
        ctx,
        http::get(rt, ctx, &status_page_path(&slug), &[])?,
        "status page",
    )
    .await?;
    validate_status_page(ctx, &page)?;
    let beats = http::send_json(
        ctx,
        http::get(rt, ctx, &heartbeat_path(&slug), &[])?,
        "heartbeats",
    )
    .await?;
    validate_heartbeats(ctx, &beats)
}

pub async fn status_page(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    slug_override: Option<&str>,
) -> HomarrResult<StatusPage> {
    let slug = slug_override
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slug(ctx));
    let (page, beats) = tokio::join!(
        http::send_json(
            ctx,
            http::get(rt, ctx, &status_page_path(&slug), &[])?,
            "status page"
        ),
        http::send_json(
            ctx,
            http::get(rt, ctx, &heartbeat_path(&slug), &[])?,
            "heartbeats"
        ),
    );
    let (page, beats) = (page?, beats?);
    validate_status_page(ctx, &page)?;
    validate_heartbeats(ctx, &beats)?;
    let external = ctx
        .external_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&ctx.url);
    let url = http::build_url(external, &format!("/status/{}", encode(&slug)), &[])?;
    Ok(map_status_page(
        &slug,
        url.to_string(),
        &page,
        &beats,
        Utc::now().timestamp_millis(),
    ))
}

fn validate_status_page(ctx: &IntegrationContext, page: &Value) -> HomarrResult<()> {
    let invalid = || {
        parse_error(
            ctx,
            "Uptime Kuma status page",
            "Invalid Uptime Kuma status page response",
        )
    };
    let groups = page
        .get("publicGroupList")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    for group in groups {
        group
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(invalid)?;
        str_of(group, "name").ok_or_else(invalid)?;
        let monitors = group
            .get("monitorList")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        for monitor in monitors {
            monitor
                .get("id")
                .and_then(Value::as_i64)
                .ok_or_else(invalid)?;
            str_of(monitor, "name").ok_or_else(invalid)?;
        }
    }
    Ok(())
}

fn validate_heartbeats(ctx: &IntegrationContext, beats: &Value) -> HomarrResult<()> {
    let invalid = || {
        parse_error(
            ctx,
            "Uptime Kuma heartbeats",
            "Invalid Uptime Kuma heartbeat response",
        )
    };
    beats
        .get("heartbeatList")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    beats
        .get("uptimeList")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    Ok(())
}

/// Uptime Kuma sends `YYYY-MM-DD HH:mm:ss(.SSS)` in UTC, sometimes with an offset.
pub fn parse_date(value: Option<&str>) -> Option<i64> {
    let value = value.filter(|v| !v.is_empty())?;
    let normalized = if value.contains('T') {
        value.to_string()
    } else {
        value.replacen(' ', "T", 1)
    };
    if let Ok(date) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(date.timestamp_millis());
    }
    if let Ok(date) = DateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f%#z") {
        return Some(date.timestamp_millis());
    }
    NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|d| d.and_utc().timestamp_millis())
}

fn map_heartbeats(raw: &Value) -> Vec<Heartbeat> {
    let mut beats: Vec<Heartbeat> = raw
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|beat| {
                    Some(Heartbeat {
                        status: monitor_status(beat.get("status")?.as_i64()?),
                        time: parse_date(str_of(beat, "time"))?,
                        ping: beat.get("ping").and_then(Value::as_f64),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    beats.sort_by_key(|b| b.time);
    let skip = beats.len().saturating_sub(MAX_HEARTBEATS);
    beats.split_off(skip)
}

fn median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied()
}

fn check_interval(heartbeats: &[Heartbeat]) -> Option<i64> {
    let diffs: Vec<i64> = heartbeats
        .windows(2)
        .map(|w| w[1].time - w[0].time)
        .collect();
    median(&diffs)
}

fn stale_after(interval: Option<i64>) -> i64 {
    interval.map_or(UPTIME_WINDOW_MS, |i| {
        MINIMUM_STALE_AGE_MS.max(i * MISSED_CHECKS_UNTIL_STALE)
    })
}

fn status_since(heartbeats: &[Heartbeat]) -> Option<i64> {
    let latest = heartbeats.last()?;
    (0..heartbeats.len().saturating_sub(1))
        .rev()
        .find(|&i| heartbeats[i].status != latest.status)
        .map(|i| heartbeats[i + 1].time)
}

fn map_monitor(
    monitor: &Value,
    heartbeats: Vec<Heartbeat>,
    uptime_ratio: Option<f64>,
    now_ms: i64,
) -> Monitor {
    let latest = heartbeats.last();
    let checked_within_window = latest.is_some_and(|l| now_ms - l.time <= UPTIME_WINDOW_MS);
    let status = match latest {
        None => MonitorStatus::Unknown,
        Some(l) if now_ms - l.time > stale_after(check_interval(&heartbeats)) => {
            MonitorStatus::Stale
        }
        Some(l) => l.status,
    };
    let status_since = (status != MonitorStatus::Stale)
        .then(|| status_since(&heartbeats))
        .flatten();
    let non_empty = |key: &str| {
        str_of(monitor, key)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let certificate_days_remaining = (monitor.get("validCert").and_then(Value::as_bool)
        == Some(true))
    .then(|| monitor.get("certExpiryDaysRemaining"))
    .flatten()
    .and_then(Value::as_f64);
    Monitor {
        id: monitor
            .get("id")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        name: str_of(monitor, "name").unwrap_or_default().to_string(),
        monitor_type: str_of(monitor, "type").map(str::to_string),
        url: non_empty("url"),
        status,
        uptime24h: uptime_ratio
            .filter(|_| checked_within_window)
            .map(|r| (r * 100.0).clamp(0.0, 100.0)),
        ping: latest.and_then(|l| l.ping),
        last_check_at: latest.map(|l| l.time),
        status_since,
        certificate_days_remaining,
        tags: monitor
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| {
                        Some(Tag {
                            name: str_of(tag, "name")?.to_string(),
                            color: str_of(tag, "color").map(str::to_string),
                            value: str_of(tag, "value")
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        heartbeats,
    }
}

fn trimmed(value: &Value, key: &str) -> Option<String> {
    str_of(value, key)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn map_incident(incident: &Value) -> Incident {
    let style = str_of(incident, "style").filter(|s| INCIDENT_STYLES.contains(s));
    Incident {
        id: incident
            .get("id")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        title: trimmed(incident, "title").unwrap_or_default(),
        content: trimmed(incident, "content"),
        style: style.unwrap_or("warning").to_string(),
        created_at: parse_date(str_of(incident, "createdDate")),
        updated_at: parse_date(str_of(incident, "lastUpdatedDate")),
    }
}

pub fn map_status_page(
    slug: &str,
    url: String,
    page: &Value,
    beats: &Value,
    now_ms: i64,
) -> StatusPage {
    let mut heartbeats_by_monitor: HashMap<i64, Vec<Heartbeat>> = HashMap::new();
    if let Some(list) = beats.get("heartbeatList").and_then(Value::as_object) {
        for (monitor_id, raw) in list {
            if let Ok(id) = monitor_id.parse::<i64>() {
                heartbeats_by_monitor.insert(id, map_heartbeats(raw));
            }
        }
    }
    let uptime_list = beats.get("uptimeList");
    let groups = page
        .get("publicGroupList")
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .map(|group| Group {
                    id: group.get("id").and_then(Value::as_i64).unwrap_or_default(),
                    name: str_of(group, "name").unwrap_or_default().to_string(),
                    monitors: group
                        .get("monitorList")
                        .and_then(Value::as_array)
                        .map(|monitors| {
                            monitors
                                .iter()
                                .map(|monitor| {
                                    let id = monitor
                                        .get("id")
                                        .and_then(Value::as_i64)
                                        .unwrap_or_default();
                                    map_monitor(
                                        monitor,
                                        heartbeats_by_monitor.get(&id).cloned().unwrap_or_default(),
                                        uptime_list
                                            .and_then(|l| l.get(format!("{id}_24")))
                                            .and_then(as_f64_number),
                                        now_ms,
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();

    // `incidents` first, then the single `incident`, unique by id (later entries win, first position kept).
    let mut incidents: Vec<&Value> = Vec::new();
    let candidates = page
        .get("incidents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(page.get("incident").filter(|i| i.is_object()));
    for candidate in candidates {
        let id = candidate.get("id").and_then(Value::as_i64);
        match incidents
            .iter()
            .position(|i| i.get("id").and_then(Value::as_i64) == id)
        {
            Some(position) => incidents[position] = candidate,
            None => incidents.push(candidate),
        }
    }

    StatusPage {
        title: page
            .pointer("/config/title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(slug)
            .to_string(),
        url,
        groups,
        incidents: incidents.into_iter().map(map_incident).collect(),
        maintenances: page
            .get("maintenanceList")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|m| Maintenance {
                        id: m.get("id").and_then(Value::as_i64).unwrap_or_default(),
                        title: trimmed(m, "title").unwrap_or_default(),
                        description: trimmed(m, "description"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn as_f64_number(value: &Value) -> Option<f64> {
    value.is_number().then(|| as_f64(value)).flatten()
}

fn is_inactive(status: MonitorStatus) -> bool {
    matches!(status, MonitorStatus::Stale | MonitorStatus::Unknown)
}

/// Monitors of all groups, unique by id (a monitor can appear in several groups).
pub fn unique_monitors(groups: &[Group]) -> Vec<&Monitor> {
    let mut seen: Vec<&Monitor> = Vec::new();
    for monitor in groups.iter().flat_map(|g| &g.monitors) {
        match seen.iter().position(|m| m.id == monitor.id) {
            Some(position) => seen[position] = monitor,
            None => seen.push(monitor),
        }
    }
    seen
}

pub fn summarize(monitors: &[&Monitor]) -> Summary {
    let mut counts: HashMap<MonitorStatus, usize> = [
        MonitorStatus::Up,
        MonitorStatus::Down,
        MonitorStatus::Pending,
        MonitorStatus::Maintenance,
        MonitorStatus::Stale,
        MonitorStatus::Unknown,
    ]
    .into_iter()
    .map(|s| (s, 0))
    .collect();
    for monitor in monitors {
        *counts.entry(monitor.status).or_default() += 1;
    }
    let active: Vec<&&Monitor> = monitors.iter().filter(|m| !is_inactive(m.status)).collect();
    let uptimes: Vec<f64> = active.iter().filter_map(|m| m.uptime24h).collect();
    let count = |status| counts.get(&status).copied().unwrap_or(0);
    let overall_status = if active.is_empty() {
        OverallStatus::Unknown
    } else if count(MonitorStatus::Down) == active.len() {
        OverallStatus::Outage
    } else if count(MonitorStatus::Down) + count(MonitorStatus::Pending) > 0 {
        OverallStatus::Degraded
    } else if count(MonitorStatus::Maintenance) > 0 {
        OverallStatus::Maintenance
    } else if count(MonitorStatus::Stale) + count(MonitorStatus::Unknown) > 0 {
        OverallStatus::Unknown
    } else {
        OverallStatus::Operational
    };
    Summary {
        total: monitors.len(),
        average_uptime24h: (!uptimes.is_empty())
            .then(|| uptimes.iter().sum::<f64>() / uptimes.len() as f64),
        overall_status,
        counts,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: i64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z

    #[test]
    fn encodes_slug_like_encode_uri_component() {
        assert_eq!(encode("my page"), "my%20page");
        assert_eq!(encode("a+b/c?d#e&f"), "a%2Bb%2Fc%3Fd%23e%26f");
        assert_eq!(encode("keep-_.!~*'()"), "keep-_.!~*'()");
        assert_eq!(encode("ü"), "%C3%BC");
        assert_eq!(status_page_path("a b"), "/api/status-page/a%20b");
        assert_eq!(heartbeat_path("a b"), "/api/status-page/heartbeat/a%20b");
    }

    #[test]
    fn parses_uptime_kuma_dates() {
        assert_eq!(parse_date(Some("2026-01-01 00:00:00")), Some(NOW));
        assert_eq!(
            parse_date(Some("2026-01-01T00:00:00.500Z")),
            Some(NOW + 500)
        );
        assert_eq!(parse_date(Some("2026-01-01T01:00:00+01:00")), Some(NOW));
        assert_eq!(parse_date(Some("nope")), None);
        assert_eq!(parse_date(None), None);
    }

    fn fixture() -> (Value, Value) {
        let page = json!({
            "config": {"title": " My Status "},
            "incident": {"id": 1, "title": "Outage", "style": "danger", "createdDate": "2026-01-01 00:00:00"},
            "incidents": [{"id": 1, "title": "Outage v2", "style": "weird"}, {"id": 2, "title": "Other"}],
            "maintenanceList": [{"id": 9, "title": " Planned ", "description": ""}],
            "publicGroupList": [{"id": 1, "name": "Core", "monitorList": [
                {"id": 10, "name": "web", "type": "http", "url": " https://x ", "validCert": true, "certExpiryDaysRemaining": 30,
                 "tags": [{"name": "env", "value": " prod "}]},
                {"id": 11, "name": "old"}
            ]}]
        });
        let minute = 60_000;
        let beat = |i: i64, status: i64| {
            let ms = NOW - (10 - i) * minute;
            let date = chrono::DateTime::from_timestamp_millis(ms)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            json!({"status": status, "time": date, "ping": 12.0 + i as f64})
        };
        let heartbeats = json!({
            "heartbeatList": {
                "10": (0..=10).map(|i| beat(i, if i < 6 { 0 } else { 1 })).collect::<Vec<_>>(),
                "11": [{"status": 1, "time": "2025-12-01 00:00:00"}, {"status": 1, "time": "2025-12-01 00:01:00"}]
            },
            "uptimeList": {"10_24": 0.995, "11_24": 1.0}
        });
        (page, heartbeats)
    }

    #[test]
    fn maps_status_page() {
        let (page, beats) = fixture();
        let result = map_status_page(
            "default",
            "https://k/status/default".into(),
            &page,
            &beats,
            NOW,
        );
        assert_eq!(result.title, "My Status");
        let web = &result.groups[0].monitors[0];
        assert_eq!(web.status, MonitorStatus::Up);
        assert_eq!(web.url.as_deref(), Some("https://x"));
        assert_eq!(web.uptime24h, Some(99.5));
        assert_eq!(web.certificate_days_remaining, Some(30.0));
        assert_eq!(web.tags[0].value.as_deref(), Some("prod"));
        assert_eq!(web.status_since, Some(NOW - 4 * 60_000));
        assert_eq!(web.heartbeats.len(), 11);
        let old = &result.groups[0].monitors[1];
        assert_eq!(old.status, MonitorStatus::Stale);
        assert_eq!(old.uptime24h, None);
        assert_eq!(old.status_since, None);
        assert_eq!(result.incidents.len(), 2);
        assert_eq!(result.incidents[0].title, "Outage");
        assert_eq!(result.incidents[0].style, "danger");
        assert_eq!(result.incidents[1].style, "warning");
        assert_eq!(result.maintenances[0].title, "Planned");
        assert_eq!(result.maintenances[0].description, None);
    }

    #[test]
    fn summarizes_monitors() {
        let (page, beats) = fixture();
        let result = map_status_page("default", String::new(), &page, &beats, NOW);
        let monitors = unique_monitors(&result.groups);
        let summary = summarize(&monitors);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.counts[&MonitorStatus::Stale], 1);
        assert_eq!(summary.average_uptime24h, Some(99.5));
        assert_eq!(summary.overall_status, OverallStatus::Unknown);
        assert_eq!(summarize(&[]).overall_status, OverallStatus::Unknown);
    }
}
