use chrono::{Duration, Utc};
use homarr_common::HomarrResult;
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use serde_json::Value;

use crate::clients::{get_json, simple_get_test};
use crate::types::{CalendarEvent, IndexerInfo, IntegrationContext, MediaOrganizerItem};

fn api_prefix(kind: IntegrationKind) -> &'static str {
    match kind {
        IntegrationKind::Lidarr
        | IntegrationKind::Readarr
        | IntegrationKind::Prowlarr
        | IntegrationKind::Bindery => "/api/v1",
        IntegrationKind::Bazarr => "/api",
        _ => "/api/v3",
    }
}

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let path = format!("{}/system/status", api_prefix(ctx.kind));
    simple_get_test(http, ctx, &path).await
}

pub async fn get_calendar_events(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    start: Option<&str>,
    end: Option<&str>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let now = Utc::now();
    let s = start
        .map(str::to_string)
        .unwrap_or_else(|| (now - Duration::days(7)).to_rfc3339());
    let e = end
        .map(str::to_string)
        .unwrap_or_else(|| (now + Duration::days(30)).to_rfc3339());
    let path = format!("{}/calendar?start={s}&end={e}", api_prefix(ctx.kind));
    let val = get_json(http, ctx, &path).await?;
    let Some(arr) = val.as_array() else {
        return Ok(Vec::new());
    };

    let color = match ctx.kind {
        IntegrationKind::Sonarr => "#35c5f4",
        IntegrationKind::Radarr => "#ffc230",
        IntegrationKind::Lidarr => "#159552",
        IntegrationKind::Readarr => "#8e2222",
        _ => "#3b82f6",
    };

    Ok(arr
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let id = item
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| idx.to_string());
            let title = item
                .get("series")
                .and_then(|s| s.get("title"))
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("Release")
                .to_string();
            let sub_title = item
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|t| *t != title)
                .map(str::to_string);
            let start_date = item
                .get("airDateUtc")
                .or_else(|| item.get("inCinemas"))
                .or_else(|| item.get("physicalRelease"))
                .or_else(|| item.get("releaseDate"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            CalendarEvent {
                id,
                title,
                sub_title,
                description: item
                    .get("overview")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                start_date,
                end_date: None,
                image_url: None,
                link: None,
                indicator_color: color.to_string(),
            }
        })
        .collect())
}

pub async fn get_indexers(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<IndexerInfo>> {
    let path = if ctx.kind == IntegrationKind::Jackett {
        "/api/v2.0/indexers?configured=true".to_string()
    } else {
        format!("{}/indexer", api_prefix(ctx.kind))
    };
    let val = get_json(http, ctx, &path).await?;
    let Some(arr) = val.as_array() else {
        return Ok(Vec::new());
    };
    Ok(arr
        .iter()
        .enumerate()
        .map(|(idx, item)| IndexerInfo {
            id: item
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| idx.to_string()),
            name: item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Indexer")
                .to_string(),
            enabled: item.get("enable").and_then(|v| v.as_bool()).unwrap_or(true),
            status: "healthy".to_string(),
            protocol: item
                .get("protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        })
        .collect())
}

pub async fn get_missing(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaOrganizerItem>> {
    let path = format!("{}/wanted/missing?page=1&pageSize=20", api_prefix(ctx.kind));
    let val = get_json(http, ctx, &path).await?;
    let records = val
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(records
        .iter()
        .enumerate()
        .map(|(idx, item)| MediaOrganizerItem {
            id: item
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| idx.to_string()),
            title: item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Missing Item")
                .to_string(),
            subtitle: item
                .get("series")
                .and_then(|s| s.get("title"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            release_date: item
                .get("airDateUtc")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
        .collect())
}

pub async fn get_service_version(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<String>> {
    let path = format!("{}/system/status", api_prefix(ctx.kind));
    let val = get_json(http, ctx, &path).await?;
    Ok(val
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string))
}
