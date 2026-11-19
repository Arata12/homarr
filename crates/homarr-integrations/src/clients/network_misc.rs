use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use reqwest::Method;
use serde_json::{json, Value};

use crate::clients::{get_json, request_json_with_auth, simple_get_test};
use crate::http_auth::resolve_declared_http_auth;
use crate::types::{CalendarEvent, IntegrationContext, NotificationItem, SmartHomeEntity};

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    match ctx.kind {
        IntegrationKind::HomeAssistant => simple_get_test(http, ctx, "/api/").await,
        IntegrationKind::Opnsense => simple_get_test(http, ctx, "/api/core/system/status").await,
        IntegrationKind::Traefik => simple_get_test(http, ctx, "/api/overview").await,
        IntegrationKind::Gluetun => simple_get_test(http, ctx, "/v1/publicip/ip").await,
        IntegrationKind::Ntfy => {
            let topic = ctx.require_secret("topic")?;
            simple_get_test(http, ctx, &format!("/{topic}/json?poll=1")).await
        }
        IntegrationKind::Gotify => simple_get_test(http, ctx, "/version").await,
        IntegrationKind::PaperlessNgx => simple_get_test(http, ctx, "/api/statistics/").await,
        IntegrationKind::Anchor => simple_get_test(http, ctx, "/api/notes").await,
        IntegrationKind::SpeedtestTracker => {
            simple_get_test(http, ctx, "/api/v1/results/latest").await
        }
        IntegrationKind::Tracearr => simple_get_test(http, ctx, "/api/v1/health").await,
        IntegrationKind::Thingport => simple_get_test(http, ctx, "/api/v1/stats").await,
        IntegrationKind::Umami => simple_get_test(http, ctx, "/websites").await,
        IntegrationKind::ArchiveTeamWarrior => simple_get_test(http, ctx, "/").await,
        IntegrationKind::UnifiController => simple_get_test(http, ctx, "/status").await,
        IntegrationKind::Wazuh => simple_get_test(http, ctx, "/").await,
        IntegrationKind::Nextcloud => simple_get_test(http, ctx, "/status.php").await,
        IntegrationKind::Ical => {
            let feed_url = ctx.require_secret("url")?;
            let resp = http
                .standard()
                .get(feed_url)
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("iCal fetch failed: {e}")))?;
            if !resp.status().is_success() {
                return Err(HomarrError::Upstream(format!(
                    "iCal HTTP {}",
                    resp.status()
                )));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub async fn get_smart_home_entities(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<SmartHomeEntity>> {
    let val = get_json(http, ctx, "/api/states").await?;
    let arr = val.as_array().cloned().unwrap_or_default();
    Ok(arr
        .into_iter()
        .take(50)
        .filter_map(|v| {
            let entity_id = v.get("entity_id")?.as_str()?.to_string();
            let domain = entity_id.split('.').next().unwrap_or("sensor").to_string();
            let state = v.get("state")?.as_str()?.to_string();
            let attrs = v.get("attributes").cloned().unwrap_or(Value::Null);
            let friendly_name = attrs
                .get("friendly_name")
                .and_then(|f| f.as_str())
                .unwrap_or(&entity_id)
                .to_string();
            let unit_of_measurement = attrs
                .get("unit_of_measurement")
                .and_then(|u| u.as_str())
                .map(str::to_string);
            Some(SmartHomeEntity {
                entity_id,
                friendly_name,
                state,
                unit_of_measurement,
                domain,
            })
        })
        .collect())
}

pub async fn execute_smart_home_action(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    entity_id: &str,
    service: &str,
) -> HomarrResult<()> {
    let auth = resolve_declared_http_auth(ctx)?;
    let domain = entity_id.split('.').next().unwrap_or("homeassistant");
    let path = format!("/api/services/{domain}/{service}");
    let _ = request_json_with_auth(
        http,
        ctx,
        Method::POST,
        &path,
        Some(json!({ "entity_id": entity_id })),
        &auth,
    )
    .await?;
    Ok(())
}

pub async fn get_notifications(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<NotificationItem>> {
    match ctx.kind {
        IntegrationKind::Gotify => {
            let val = get_json(http, ctx, "/message?limit=20").await?;
            let msgs = val
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(msgs
                .iter()
                .map(|m| NotificationItem {
                    id: m
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                    title: m
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Notification")
                        .to_string(),
                    body: m
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    priority: m
                        .get("priority")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                        .to_string(),
                    created_at: m
                        .get("date")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect())
        }
        IntegrationKind::Ntfy => {
            let topic = ctx.require_secret("topic")?;
            let auth = resolve_declared_http_auth(ctx)?;
            let url = crate::clients::build_request_url(
                ctx.base_url(),
                &format!("/{topic}/json?poll=1"),
                &auth.query,
            )?;
            let mut req = http.standard().get(url);
            for (k, v) in auth.headers {
                req = req.header(k, v);
            }
            let text = req
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("ntfy failed: {e}")))?
                .text()
                .await
                .unwrap_or_default();
            let mut out = Vec::new();
            for line in text.lines() {
                if let Ok(val) = serde_json::from_str::<Value>(line) {
                    if val.get("event").and_then(|v| v.as_str()) == Some("message") {
                        out.push(NotificationItem {
                            id: val
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            title: val
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or(topic)
                                .to_string(),
                            body: val
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            priority: val
                                .get("priority")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(3)
                                .to_string(),
                            created_at: String::new(),
                        });
                    }
                }
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn get_ical_events(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<CalendarEvent>> {
    let feed_url = ctx.require_secret("url")?;
    let text = http
        .standard()
        .get(feed_url)
        .send()
        .await
        .map_err(|e| HomarrError::Upstream(format!("iCal fetch failed: {e}")))?
        .text()
        .await
        .map_err(|e| HomarrError::Upstream(format!("iCal read failed: {e}")))?;

    Ok(parse_ics_events(&text))
}

pub fn parse_ics_events(ics: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    let mut in_event = false;
    let mut uid = String::new();
    let mut summary = String::new();
    let mut description = None;
    let mut dtstart = String::new();

    for raw_line in ics.lines() {
        let line = raw_line.trim();
        if line == "BEGIN:VEVENT" {
            in_event = true;
            uid.clear();
            summary.clear();
            description = None;
            dtstart.clear();
        } else if line == "END:VEVENT" && in_event {
            in_event = false;
            if !summary.is_empty() && !dtstart.is_empty() {
                events.push(CalendarEvent {
                    id: if uid.is_empty() {
                        format!("ical-{}", events.len())
                    } else {
                        uid.clone()
                    },
                    title: summary.clone(),
                    sub_title: None,
                    description: description.clone(),
                    start_date: normalize_ics_date(&dtstart),
                    end_date: None,
                    image_url: None,
                    link: None,
                    indicator_color: "#6366f1".into(),
                });
            }
        } else if in_event {
            if let Some((k, v)) = line.split_once(':') {
                let key_name = k.split(';').next().unwrap_or("");
                match key_name {
                    "UID" => uid = v.to_string(),
                    "SUMMARY" => summary = v.to_string(),
                    "DESCRIPTION" => description = Some(v.replace("\\n", "\n")),
                    "DTSTART" => dtstart = v.to_string(),
                    _ => {}
                }
            }
        }
    }
    events
}

fn normalize_ics_date(raw: &str) -> String {
    if raw.len() >= 8 {
        let y = &raw[0..4];
        let m = &raw[4..6];
        let d = &raw[6..8];
        if raw.len() >= 15 && raw.as_bytes()[8] == b'T' {
            let hh = &raw[9..11];
            let mm = &raw[11..13];
            let ss = &raw[13..15];
            return format!("{y}-{m}-{d}T{hh}:{mm}:{ss}Z");
        }
        return format!("{y}-{m}-{d}T00:00:00Z");
    }
    raw.to_string()
}
