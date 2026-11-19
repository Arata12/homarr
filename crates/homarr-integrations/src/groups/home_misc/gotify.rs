//! Gotify: notifications (`gotify-integration.ts`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::Value;

use super::util::{basic_auth, external_url, parse_json, q, send, url};
use crate::capabilities::notifications::{Notification, NotificationSource};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    id: i64,
    appid: i64,
    date: String,
    title: String,
    message: String,
    /// `extras` is `nullish().catch(undefined)` in TS: anything unexpected is ignored.
    #[serde(default)]
    extras: Value,
}

#[derive(Debug, Deserialize)]
struct Application {
    id: i64,
    name: String,
}

fn headers(ctx: &IntegrationContext) -> HomarrResult<Vec<(&'static str, String)>> {
    let username = ctx.require_secret("username")?;
    let password = ctx.require_secret("password")?;
    Ok(vec![("Authorization", basic_auth(username, password))])
}

fn apply(
    request: reqwest::RequestBuilder,
    headers: &[(&'static str, String)],
) -> reqwest::RequestBuilder {
    headers.iter().fold(request, |request, (name, value)| {
        request.header(*name, value)
    })
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let headers = headers(ctx)?;
    send(
        ctx,
        apply(rt.http.standard().get(url(ctx, "/health", &[])?), &headers),
    )
    .await?;
    Ok(())
}

/// Applications are optional enrichment; any failure yields an empty map.
async fn applications_by_id(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    headers: &[(&'static str, String)],
) -> HashMap<i64, Application> {
    let fetch = async {
        let response = send(
            ctx,
            apply(
                rt.http.standard().get(url(ctx, "/application", &[])?),
                headers,
            ),
        )
        .await?;
        let text = response
            .text()
            .await
            .map_err(|err| crate::error::request_error(ctx, &err))?;
        parse_json::<Vec<Application>>(ctx, "Gotify applications", &text)
    };
    match fetch.await {
        Ok(apps) => apps.into_iter().map(|app| (app.id, app)).collect(),
        Err(_) => HashMap::new(),
    }
}

fn messages_url(ctx: &IntegrationContext, application_id: i64) -> HomarrResult<String> {
    let mut link = external_url(ctx, "/")?;
    link.set_fragment(Some(&format!("/messages/{application_id}")));
    Ok(link.to_string())
}

pub async fn notifications(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Notification>> {
    let headers = headers(ctx)?;
    let (messages_response, applications) = tokio::join!(
        send(
            ctx,
            apply(
                rt.http
                    .standard()
                    .get(url(ctx, "/message", &[("limit", q(100))])?),
                &headers
            )
        ),
        applications_by_id(rt, ctx, &headers),
    );
    let text = messages_response?
        .text()
        .await
        .map_err(|err| crate::error::request_error(ctx, &err))?;
    let messages: MessagesResponse = serde_json::from_str(&text)
        .map_err(|err| HomarrError::Upstream(format!("Failed to parse Gotify response: {err}")))?;
    map_messages(ctx, messages.messages, &applications)
}

fn map_messages(
    ctx: &IntegrationContext,
    messages: Vec<Message>,
    applications: &HashMap<i64, Application>,
) -> HomarrResult<Vec<Notification>> {
    messages
        .into_iter()
        .map(|message| {
            let time = DateTime::parse_from_rfc3339(&message.date)
                .map_err(|err| crate::error::parse_error(ctx, "Gotify message date", err))?
                .with_timezone(&Utc);
            let content_type = message
                .extras
                .get("client::display")
                .and_then(|display| display.get("contentType"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(Notification {
                id: message.id.to_string(),
                time,
                title: message.title,
                body: message.message,
                content_type,
                href: Some(messages_url(ctx, message.appid)?),
                // The TS implementation resolves the application icon through the image proxy, which is not
                // reachable from this crate (the icon request needs the credentials).
                source: applications
                    .get(&message.appid)
                    .map(|application| NotificationSource {
                        name: application.name.clone(),
                        icon_url: None,
                    }),
            })
        })
        .collect()
}

/// `deleteNotificationAsync`.
pub async fn delete_notification(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    id: &str,
) -> HomarrResult<()> {
    let headers = headers(ctx)?;
    let encoded = url::form_urlencoded::byte_serialize(id.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let path = format!("/message/{encoded}");
    send(
        ctx,
        apply(rt.http.standard().delete(url(ctx, &path, &[])?), &headers),
    )
    .await?;
    Ok(())
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    match operation {
        "deleteNotification" => {
            let id = super::util::input_str(input, "id")
                .ok_or_else(|| HomarrError::BadRequest("id is required".into()))?;
            delete_notification(rt, ctx, id).await?;
            Ok(Value::Null)
        }
        other => Err(super::util::unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "g".into(),
            name: "Gotify".into(),
            url: "http://gotify.local".into(),
            external_url: Some("https://gotify.example.com/".into()),
            kind: IntegrationKind::Gotify,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn maps_messages_with_application_and_content_type() {
        let body = json!({"messages": [
            {"id": 7, "appid": 2, "date": "2018-02-27T19:36:10.5045044+01:00", "title": "T", "message": "M",
             "extras": {"client::display": {"contentType": "text/markdown"}}},
            {"id": 8, "appid": 9, "date": "2018-02-27T19:36:10Z", "title": "U", "message": "N", "extras": "broken"}
        ]});
        let parsed: MessagesResponse = serde_json::from_value(body).unwrap();
        let apps = HashMap::from([(
            2,
            Application {
                id: 2,
                name: "App".into(),
            },
        )]);
        let out = map_messages(&ctx(), parsed.messages, &apps).unwrap();
        assert_eq!(out[0].id, "7");
        assert_eq!(out[0].content_type.as_deref(), Some("text/markdown"));
        assert_eq!(
            out[0].href.as_deref(),
            Some("https://gotify.example.com/#/messages/2")
        );
        assert_eq!(out[0].source.as_ref().unwrap().name, "App");
        assert!(out[1].source.is_none() && out[1].content_type.is_none());
    }
}
