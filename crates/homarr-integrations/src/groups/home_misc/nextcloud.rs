//! Nextcloud: CalDAV calendar and notifications (`nextcloud.integration.ts`, `nextcloud-url.ts`).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use reqwest::Method;
use serde::Deserialize;
use url::Url;

use super::ics::{self, VEvent};
use super::util::{basic_auth, external_url, q, read_json, send, url as integration_url};
use crate::capabilities::calendar::{CalendarEvent, CalendarLink};
use crate::capabilities::notifications::Notification;
use crate::error::{check_status, parse_error, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// `createNextcloudCalendarServerUrl`.
pub fn calendar_server_url(integration_url: &str) -> HomarrResult<String> {
    let mut url = Url::parse(integration_url).map_err(|err| {
        HomarrError::BadRequest(format!(
            "Invalid integration URL '{integration_url}': {err}"
        ))
    })?;
    let path = url.path().trim_end_matches('/').to_string();
    let dav_path = "/remote.php/dav";
    let new_path = if path.ends_with(dav_path) {
        format!("{path}/")
    } else {
        format!("{path}{dav_path}/")
    };
    url.set_path(&new_path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn auth(ctx: &IntegrationContext) -> HomarrResult<String> {
    Ok(basic_auth(
        ctx.require_secret("username")?,
        ctx.require_secret("password")?,
    ))
}

// ---- WebDAV ------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct DavResponse {
    href: String,
    is_calendar: bool,
    components: Vec<String>,
    /// First `href` found inside `current-user-principal` / `calendar-home-set`.
    nested_href: Option<String>,
    calendar_data: Option<String>,
}

fn local_name(name: &[u8]) -> String {
    let text = String::from_utf8_lossy(name);
    text.rsplit(':').next().unwrap_or_default().to_lowercase()
}

/// Parses a `207 Multi-Status` body, ignoring namespace prefixes.
fn parse_multistatus(xml: &str) -> HomarrResult<Vec<DavResponse>> {
    let mut reader = Reader::from_str(xml);
    let mut responses = Vec::new();
    let mut current: Option<DavResponse> = None;
    let mut path: Vec<String> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = local_name(element.name().as_ref());
                if name == "response" {
                    current = Some(DavResponse::default());
                }
                if let Some(response) = current.as_mut() {
                    if name == "comp"
                        && path
                            .last()
                            .is_some_and(|parent| parent == "supported-calendar-component-set")
                    {
                        for attribute in element.attributes().flatten() {
                            if attribute.key.as_ref() == b"name" {
                                response
                                    .components
                                    .push(String::from_utf8_lossy(&attribute.value).to_uppercase());
                            }
                        }
                    }
                }
                path.push(name);
            }
            Ok(Event::Empty(element)) => {
                let name = local_name(element.name().as_ref());
                if let Some(response) = current.as_mut() {
                    if name == "calendar"
                        && path.last().is_some_and(|parent| parent == "resourcetype")
                    {
                        response.is_calendar = true;
                    }
                    if name == "comp"
                        && path
                            .last()
                            .is_some_and(|parent| parent == "supported-calendar-component-set")
                    {
                        for attribute in element.attributes().flatten() {
                            if attribute.key.as_ref() == b"name" {
                                response
                                    .components
                                    .push(String::from_utf8_lossy(&attribute.value).to_uppercase());
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(text)) => {
                let value = text.unescape().map(|v| v.into_owned()).unwrap_or_default();
                store_text(&mut current, &path, &value);
            }
            Ok(Event::CData(data)) => {
                let value = String::from_utf8_lossy(&data).into_owned();
                store_text(&mut current, &path, &value);
            }
            Ok(Event::End(element)) => {
                let name = local_name(element.name().as_ref());
                path.pop();
                if name == "response" {
                    if let Some(response) = current.take() {
                        responses.push(response);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                return Err(HomarrError::Upstream(format!(
                    "Invalid CalDAV response: {err}"
                )))
            }
        }
    }
    Ok(responses)
}

fn store_text(current: &mut Option<DavResponse>, path: &[String], value: &str) {
    let Some(response) = current.as_mut() else {
        return;
    };
    match path.last().map(String::as_str) {
        Some("href") => {
            let parent = path
                .len()
                .checked_sub(2)
                .and_then(|index| path.get(index))
                .map(String::as_str);
            if matches!(parent, Some("current-user-principal" | "calendar-home-set")) {
                response
                    .nested_href
                    .get_or_insert_with(|| value.trim().to_string());
            } else if parent == Some("response") {
                response.href = value.trim().to_string();
            }
        }
        Some("calendar-data") => {
            response
                .calendar_data
                .get_or_insert_with(String::new)
                .push_str(value);
        }
        _ => {}
    }
}

async fn dav(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: &str,
    url: &Url,
    depth: &str,
    body: String,
) -> HomarrResult<Vec<DavResponse>> {
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|err| HomarrError::Internal(format!("Invalid method: {err}")))?;
    let request = rt
        .http
        .standard()
        .request(method, url.clone())
        .header("Authorization", auth(ctx)?)
        .header("Depth", depth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body);
    let response = check_status(ctx, super::util::send_raw(ctx, request).await?)?;
    let text = response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    parse_multistatus(&text)
}

fn resolve(base: &Url, href: &str) -> HomarrResult<Url> {
    let resolved = base
        .join(href)
        .map_err(|err| HomarrError::Upstream(format!("Invalid CalDAV href '{href}': {err}")))?;
    if resolved.origin() != base.origin() {
        return Err(HomarrError::Upstream(format!(
            "Refusing cross-origin CalDAV href '{href}'"
        )));
    }
    Ok(resolved)
}

const PRINCIPAL_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
const HOME_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
const CALENDARS_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:resourcetype/><d:displayname/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;

/// tsdav `login()`: discovers the calendar home of the user.
async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Url> {
    let server = Url::parse(&calendar_server_url(&ctx.url)?)
        .map_err(|err| HomarrError::BadRequest(format!("Invalid integration URL: {err}")))?;
    let principal = dav(rt, ctx, "PROPFIND", &server, "0", PRINCIPAL_BODY.into()).await?;
    let principal_href = principal
        .iter()
        .find_map(|response| response.nested_href.clone())
        .ok_or_else(|| {
            HomarrError::Upstream(format!("{} did not report a CalDAV principal", ctx.name))
        })?;
    let principal_url = resolve(&server, &principal_href)?;

    let home = dav(rt, ctx, "PROPFIND", &principal_url, "0", HOME_BODY.into()).await?;
    let home_href = home
        .iter()
        .find_map(|response| response.nested_href.clone())
        .ok_or_else(|| {
            HomarrError::Upstream(format!(
                "{} did not report a CalDAV calendar home",
                ctx.name
            ))
        })?;
    resolve(&principal_url, &home_href)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    login(rt, ctx).await.map(|_| ())
}

fn caldav_time(time: DateTime<Utc>) -> String {
    time.format("%Y%m%dT%H%M%SZ").to_string()
}

async fn fetch_calendar_objects(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    calendar_url: &Url,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<(Url, String)>> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
        caldav_time(start),
        caldav_time(end)
    );
    let responses = dav(rt, ctx, "REPORT", calendar_url, "1", body).await?;
    responses
        .into_iter()
        .filter_map(|response| {
            let data = response.calendar_data?;
            Some(resolve(calendar_url, &response.href).map(|url| (url, data)))
        })
        .collect()
}

pub async fn calendar_events(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let home = login(rt, ctx).await?;
    let listing = dav(rt, ctx, "PROPFIND", &home, "1", CALENDARS_BODY.into()).await?;
    let calendars: Vec<Url> = listing
        .into_iter()
        .filter(|response| response.is_calendar)
        .filter(|response| {
            response.components.is_empty() || response.components.iter().any(|c| c == "VEVENT")
        })
        .map(|response| resolve(&home, &response.href))
        .collect::<HomarrResult<_>>()?;

    let objects = futures_util::future::try_join_all(
        calendars
            .iter()
            .map(|calendar| fetch_calendar_objects(rt, ctx, calendar, start, end)),
    )
    .await?;

    let mut events = Vec::new();
    for (object_url, data) in objects.into_iter().flatten() {
        events.extend(object_events(ctx, &object_url, &data, start, end)?);
    }
    Ok(events)
}

/// Converts the first VEVENT of a calendar object (and its recurrences inside the window).
fn object_events(
    ctx: &IntegrationContext,
    object_url: &Url,
    data: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let calendar = match ics::parse(data) {
        Ok(calendar) => calendar,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(event) = calendar
        .children_named("VEVENT")
        .find_map(VEvent::from_component)
    else {
        return Ok(Vec::new());
    };
    let length = event.length(true);
    let slug = URL_SAFE_NO_PAD.encode(object_url.path());

    let starts = if event.is_recurring() {
        let (starts, limited) = match ics::occurrences(&event, (start - length, end), true) {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(error = %err, "Ignoring recurrence of Nextcloud event");
                (vec![event.start.to_utc(true)], false)
            }
        };
        if limited {
            tracing::warn!("Nextcloud recurring event expansion was capped");
        }
        starts
            .into_iter()
            .filter(|occurrence| *occurrence <= end && *occurrence + length >= start)
            .collect()
    } else {
        vec![event.start.to_utc(true)]
    };

    starts
        .into_iter()
        .map(|start_date| {
            let millis = start_date.timestamp_millis();
            let link = external_url(
                ctx,
                &format!(
                    "/apps/calendar/timeGridWeek/now/edit/sidebar/{slug}/{}",
                    millis as f64 / 1000.0
                ),
            )?;
            Ok(CalendarEvent {
                title: event.summary.clone(),
                sub_title: None,
                description: (!event.description.is_empty()).then(|| event.description.clone()),
                start_date,
                end_date: Some(start_date + length),
                is_all_day: None,
                image: None,
                location: (!event.location.is_empty()).then(|| event.location.clone()),
                metadata: None,
                indicator_color: event.color.clone().unwrap_or_else(|| "#ff8600".into()),
                links: vec![CalendarLink {
                    name: "Nextcloud".into(),
                    is_dark: true,
                    href: link.to_string(),
                    color: None,
                    logo: Some("/images/apps/nextcloud.svg".into()),
                }],
            })
        })
        .collect()
}

// ---- notifications -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct NotificationsResponse {
    ocs: Ocs,
}

#[derive(Debug, Deserialize)]
struct Ocs {
    data: Vec<RawNotification>,
}

#[derive(Debug, Deserialize)]
struct RawNotification {
    notification_id: i64,
    datetime: String,
    subject: String,
    message: String,
}

pub async fn notifications(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Notification>> {
    let url = integration_url(
        ctx,
        "/ocs/v2.php/apps/notifications/api/v2/notifications",
        &[("format", q("json"))],
    )?;
    let request = rt
        .http
        .standard()
        .get(url)
        .header("Authorization", auth(ctx)?)
        .header("OCS-APIRequest", "true");
    let response = send(ctx, request).await?;
    let parsed: NotificationsResponse = read_json(ctx, "Nextcloud notifications", response).await?;
    map_notifications(ctx, parsed)
}

fn map_notifications(
    ctx: &IntegrationContext,
    response: NotificationsResponse,
) -> HomarrResult<Vec<Notification>> {
    response
        .ocs
        .data
        .into_iter()
        .map(|notification| {
            let time = DateTime::parse_from_rfc3339(&notification.datetime)
                .map_err(|err| parse_error(ctx, "Nextcloud notification date", err))?
                .with_timezone(&Utc);
            Ok(Notification {
                id: notification.notification_id.to_string(),
                time,
                title: notification.subject,
                body: notification.message,
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

    use chrono::TimeZone;
    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "n".into(),
            name: "Nextcloud".into(),
            url: "https://cloud.example.com".into(),
            external_url: Some("https://cloud.example.com".into()),
            kind: IntegrationKind::Nextcloud,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn calendar_server_url_matches_dav_endpoint() {
        for (input, expected) in [
            (
                "https://cloud.example.com",
                "https://cloud.example.com/remote.php/dav/",
            ),
            (
                "https://example.com/nextcloud",
                "https://example.com/nextcloud/remote.php/dav/",
            ),
            (
                "https://example.com/nextcloud/",
                "https://example.com/nextcloud/remote.php/dav/",
            ),
            (
                "https://example.com/nextcloud/remote.php/dav",
                "https://example.com/nextcloud/remote.php/dav/",
            ),
            (
                "https://example.com/nextcloud?x=1#frag",
                "https://example.com/nextcloud/remote.php/dav/",
            ),
        ] {
            assert_eq!(calendar_server_url(input).unwrap(), expected, "{input}");
        }
    }

    #[test]
    fn parses_discovery_and_calendar_listing() {
        let principal = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/remote.php/dav/</d:href><d:propstat><d:prop><d:current-user-principal><d:href>/remote.php/dav/principals/users/bob/</d:href></d:current-user-principal></d:prop></d:propstat></d:response></d:multistatus>"#;
        let parsed = parse_multistatus(principal).unwrap();
        assert_eq!(
            parsed[0].nested_href.as_deref(),
            Some("/remote.php/dav/principals/users/bob/")
        );

        let listing = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/calendars/bob/personal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VEVENT"/><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/bob/inbox/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#;
        let parsed = parse_multistatus(listing).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].is_calendar && !parsed[1].is_calendar);
        assert_eq!(parsed[0].components, vec!["VEVENT", "VTODO"]);
        assert_eq!(parsed[0].href, "/remote.php/dav/calendars/bob/personal/");
    }

    #[test]
    fn extracts_calendar_data() {
        let report = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/calendars/bob/personal/e.ics</d:href><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:e
SUMMARY:Lunch &amp; learn
DTSTART:20250110T120000Z
DTEND:20250110T130000Z
COLOR:#112233
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#;
        let parsed = parse_multistatus(report).unwrap();
        let data = parsed[0].calendar_data.clone().unwrap();
        let object_url =
            Url::parse("https://cloud.example.com/remote.php/dav/calendars/bob/personal/e.ics")
                .unwrap();
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
        let events = object_events(&ctx(), &object_url, &data, start, end).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Lunch & learn");
        assert_eq!(events[0].indicator_color, "#112233");
        assert_eq!(events[0].links[0].name, "Nextcloud");
        let slug = URL_SAFE_NO_PAD.encode("/remote.php/dav/calendars/bob/personal/e.ics");
        assert_eq!(
            events[0].links[0].href,
            format!(
                "https://cloud.example.com/apps/calendar/timeGridWeek/now/edit/sidebar/{slug}/{}",
                events[0].start_date.timestamp()
            )
        );
    }

    #[test]
    fn maps_notifications() {
        let response: NotificationsResponse = serde_json::from_str(
            r#"{"ocs":{"data":[{"notification_id":5,"datetime":"2025-01-02T03:04:05+00:00","subject":"S","message":"M"}]}}"#,
        )
        .unwrap();
        let out = map_notifications(&ctx(), response).unwrap();
        assert_eq!(out[0].id, "5");
        assert_eq!(
            out[0].time,
            Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap()
        );
    }

    #[test]
    fn resolve_rejects_cross_origin_href() {
        let base = Url::parse("https://cloud.example.com/remote.php/dav").unwrap();
        assert!(resolve(&base, "https://attacker.example/steal").is_err());
        assert!(resolve(&base, "//attacker.example/steal").is_err());
        assert_eq!(
            resolve(&base, "/remote.php/dav/principals/users/bob/")
                .unwrap()
                .as_str(),
            "https://cloud.example.com/remote.php/dav/principals/users/bob/"
        );
    }
}
