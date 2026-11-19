//! ntfy: notifications (`ntfy-integration.ts`).

use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::Value;

use super::util::{external_url, q, send, url};
use crate::capabilities::notifications::Notification;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Deserialize)]
struct Event {
    event: String,
    id: String,
    time: f64,
    topic: String,
    #[serde(default)]
    sequence_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn auth_headers(ctx: &IntegrationContext) -> Vec<(&'static str, String)> {
    match ctx.secret("apiKey") {
        Some(key) => vec![("Authorization", format!("Bearer {key}"))],
        None => Vec::new(),
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let mut request = rt.http.standard().get(url(ctx, "/v1/account", &[])?);
    for (name, value) in auth_headers(ctx) {
        request = request.header(name, value);
    }
    send(ctx, request).await?;
    Ok(())
}

pub async fn notifications(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Notification>> {
    let topic = ctx.require_secret("topic")?;
    let encoded = url::form_urlencoded::byte_serialize(topic.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let mut request =
        rt.http
            .standard()
            .get(url(ctx, &format!("/{encoded}/json"), &[("poll", q(1))])?);
    for (name, value) in auth_headers(ctx) {
        request = request.header(name, value);
    }
    let text = send(ctx, request)
        .await?
        .text()
        .await
        .map_err(|err| crate::error::request_error(ctx, &err))?;
    let events = parse_events(&text)?;
    map_events(ctx, events)
}

/// Response is provided as individual lines of JSON; events other than message/message_delete are ignored.
fn parse_events(text: &str) -> HomarrResult<Vec<Event>> {
    let mut events = Vec::new();
    for line in text.split('\n').filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|err| HomarrError::Upstream(format!("Invalid ntfy response line: {err}")))?;
        // Clear marks a message as read without replacing its content, so it is ignored like any other event.
        if matches!(
            value.get("event").and_then(Value::as_str),
            Some("message" | "message_delete")
        ) {
            let event: Event = serde_json::from_value(value).map_err(|err| {
                HomarrError::Upstream(format!("Failed to parse ntfy event: {err}"))
            })?;
            events.push(event);
        }
    }
    Ok(events)
}

fn map_events(ctx: &IntegrationContext, events: Vec<Event>) -> HomarrResult<Vec<Notification>> {
    // Keyed by (topic, sequence id or id); a later event of the same sequence replaces the earlier one while
    // replay order is preserved (delete then re-insert, like the JS Map).
    let mut ordered: Vec<((String, String), Event)> = Vec::new();
    for event in events {
        let sequence = event
            .sequence_id
            .clone()
            .filter(|sequence| !sequence.is_empty())
            .unwrap_or_else(|| event.id.clone());
        let key = (event.topic.clone(), sequence);
        ordered.retain(|(existing, _)| *existing != key);
        if event.event == "message" {
            ordered.push((key, event));
        }
    }
    ordered
        .into_iter()
        .map(|(_, event)| {
            let topic_url = external_url(ctx, &format!("/{}", event.topic))?;
            let fallback_title = format!(
                "{}{}",
                topic_url.host_str().unwrap_or_default(),
                topic_url.path()
            );
            let millis = (event.time * 1000.0) as i64;
            Ok(Notification {
                id: event.id,
                time: DateTime::<Utc>::from_timestamp_millis(millis).unwrap_or_default(),
                title: event.title.unwrap_or(fallback_title),
                body: event.message.unwrap_or_default(),
                content_type: None,
                href: None,
                source: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "n".into(),
            name: "ntfy".into(),
            url: "https://ntfy.example.com".into(),
            external_url: None,
            kind: IntegrationKind::Ntfy,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn replays_sequences_and_deletions() {
        let text = [
            r#"{"id":"a","time":1700000000,"event":"open","topic":"t"}"#,
            r#"{"id":"a","time":1700000001,"event":"message","topic":"t","message":"v1","title":"Hello"}"#,
            r#"{"id":"b","time":1700000002,"event":"message","topic":"t","sequence_id":"a","message":"v2"}"#,
            r#"{"id":"c","time":1700000003,"event":"message","topic":"t","message":"gone"}"#,
            r#"{"id":"d","time":1700000004,"event":"message_delete","topic":"t","sequence_id":"c"}"#,
            r#"{"id":"e","time":1700000005,"event":"message","topic":"t","message":"kept"}"#,
            "",
        ]
        .join("\n");
        let out = map_events(&ctx(), parse_events(&text).unwrap()).unwrap();
        let ids: Vec<_> = out.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["b", "e"]);
        assert_eq!(out[0].title, "ntfy.example.com/t");
        assert_eq!(out[0].body, "v2");
    }
}
