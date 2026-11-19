//! iCal feed calendar (`ical-integration.ts`).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use homarr_common::{HomarrError, HomarrResult};

use super::ics::{self, VEvent};
use super::util::{send, send_raw};
use crate::capabilities::calendar::CalendarEvent;
use crate::error::{check_status, parse_error, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

fn to_calendar_event(event: &VEvent, start: DateTime<Utc>, end: DateTime<Utc>) -> CalendarEvent {
    let non_empty = |value: &str| (!value.is_empty()).then(|| value.to_string());
    CalendarEvent {
        title: event.summary.clone(),
        sub_title: None,
        description: non_empty(&event.description),
        start_date: start,
        end_date: Some(end),
        is_all_day: Some(event.start.is_date()),
        image: None,
        location: non_empty(&event.location),
        metadata: None,
        indicator_color: "red".into(),
        links: Vec::new(),
    }
}

/// Start/end of the occurrence `occurrence_start` of a recurring `master`, taking exceptions into account
/// (`ICAL.Event.getOccurrenceDetails`). Returns the item the occurrence belongs to, its recurrence id and bounds.
fn occurrence_details<'a>(
    master: &'a VEvent,
    exceptions: &'a [&'a VEvent],
    occurrence_start: DateTime<Utc>,
) -> (&'a VEvent, DateTime<Utc>, DateTime<Utc>) {
    let exact = exceptions.iter().find(|exception| {
        exception
            .recurrence_id
            .is_some_and(|id| id.to_utc(false) == occurrence_start)
    });
    if let Some(exception) = exact {
        let start = exception.start.to_utc(false);
        return (exception, start, start + exception.length(false));
    }
    // RANGE=THISANDFUTURE: the latest override at or before this occurrence applies, shifted by its offset.
    let future = exceptions
        .iter()
        .filter(|exception| {
            exception.range_this_and_future
                && exception
                    .recurrence_id
                    .is_some_and(|id| id.to_utc(false) <= occurrence_start)
        })
        .max_by_key(|exception| exception.recurrence_id.map(|id| id.to_utc(false)));
    if let Some(exception) = future {
        let recurrence_id = exception
            .recurrence_id
            .map_or(occurrence_start, |id| id.to_utc(false));
        let offset = exception.start.to_utc(false) - recurrence_id;
        let start = occurrence_start + offset;
        return (exception, start, start + exception.length(false));
    }
    (
        master,
        occurrence_start,
        occurrence_start + master.length(false),
    )
}

/// Port of `ICalIntegration.getCalendarEventsAsync` on already downloaded feed text.
pub fn events_from_ics(
    text: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let calendar = ics::parse(text)?;
    let vevents: Vec<VEvent> = calendar
        .children_named("VEVENT")
        .filter_map(VEvent::from_component)
        .collect();

    let (exceptions, masters): (Vec<&VEvent>, Vec<&VEvent>) = vevents
        .iter()
        .partition(|event| event.is_recurrence_exception());
    let mut exceptions_by_uid: HashMap<&str, Vec<&VEvent>> = HashMap::new();
    for exception in &exceptions {
        exceptions_by_uid
            .entry(exception.uid.as_str())
            .or_default()
            .push(exception);
    }

    let mut events = Vec::new();
    let mut emitted_keys: HashSet<String> = HashSet::new();
    let mut emitted_exceptions: HashSet<*const VEvent> = HashSet::new();

    for master in masters {
        let related = exceptions_by_uid
            .get(master.uid.as_str())
            .cloned()
            .unwrap_or_default();
        if !master.is_recurring() {
            let event_start = master.start.to_utc(false);
            let event_end = event_start + master.length(false);
            if event_start > end || event_end < start {
                continue;
            }
            events.push(to_calendar_event(master, event_start, event_end));
            continue;
        }

        let window = (start - master.length(false), end);
        let (starts, limited) = match ics::occurrences(master, window, false) {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(error = %err, uid = %master.uid, "Ignoring recurrence of iCal event");
                (vec![master.start.to_utc(false)], false)
            }
        };
        if limited {
            tracing::warn!(
                uid = %master.uid,
                "iCal integration stopped expanding a recurring event after {} occurrences; some occurrences in the requested window may be missing",
                ics::MAX_OCCURRENCES
            );
        }

        for occurrence_start in starts {
            if occurrence_start > end {
                break;
            }
            let (item, item_start, item_end) =
                occurrence_details(master, &related, occurrence_start);
            if item_start > end || item_end < start {
                continue;
            }
            if item.is_recurrence_exception() {
                // Keyed on the occurrence's recurrence id, not the item's: for a RANGE=THISANDFUTURE override
                // those collapse to the same value for every later occurrence, which would drop them.
                let key = format!("{}:{}", item.uid, occurrence_start.to_rfc3339());
                if !emitted_keys.insert(key) {
                    continue;
                }
                emitted_exceptions.insert(std::ptr::from_ref(item));
            }
            events.push(to_calendar_event(item, item_start, item_end));
        }
    }

    for exception in exceptions {
        if emitted_exceptions.contains(&std::ptr::from_ref(exception)) {
            continue;
        }
        let recurrence_id = exception
            .recurrence_id
            .map_or_else(|| exception.start.to_utc(false), |id| id.to_utc(false));
        if emitted_keys.contains(&format!("{}:{}", exception.uid, recurrence_id.to_rfc3339())) {
            continue;
        }
        let event_start = exception.start.to_utc(false);
        let event_end = event_start + exception.length(false);
        if event_start > end || event_end < start {
            continue;
        }
        events.push(to_calendar_event(exception, event_start, event_end));
    }

    Ok(events)
}

fn parse_feed_url(ctx: &IntegrationContext) -> HomarrResult<url::Url> {
    let raw = ctx.require_secret("url")?.trim();
    let parsed = url::Url::parse(raw).map_err(|_| {
        HomarrError::BadRequest(format!("Invalid calendar feed URL for {}", ctx.name))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        return Err(HomarrError::BadRequest(format!(
            "Calendar feed URL for {} must use http:// or https://",
            ctx.name
        )));
    }
    Ok(parsed)
}

async fn download(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
    // The feed URL is the secret; it is private and never part of any message.
    let feed_url = parse_feed_url(ctx)?;
    let response = send(ctx, rt.http.standard().get(feed_url)).await?;
    response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))
}

pub async fn calendar_events(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let text = download(rt, ctx).await?;
    events_from_ics(&text, start, end).map_err(|err| parse_error(ctx, "iCal feed", err))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let feed_url = parse_feed_url(ctx)?;
    let response = check_status(ctx, send_raw(ctx, rt.http.standard().get(feed_url)).await?)?;
    let text = response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    let calendar = ics::parse(&text).map_err(|err| parse_error(ctx, "calendar", err))?;
    if calendar.children_named("VEVENT").next().is_none() {
        return Err(parse_error(ctx, "calendar", "No events found"));
    }
    Ok(())
}

/// Legacy consumers only know a single flat window; kept for the adapters.
pub fn default_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (now - Duration::days(31), now + Duration::days(93))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, day, hour, 0, 0).unwrap()
    }

    const FEED: &str = "BEGIN:VCALENDAR
BEGIN:VEVENT
UID:series
SUMMARY:Standup
DTSTART:20250106T090000Z
DTEND:20250106T093000Z
RRULE:FREQ=DAILY;COUNT=5
END:VEVENT
BEGIN:VEVENT
UID:series
SUMMARY:Standup (moved)
DTSTART:20250108T150000Z
DTEND:20250108T153000Z
RECURRENCE-ID:20250108T090000Z
END:VEVENT
BEGIN:VEVENT
UID:orphan
SUMMARY:Orphan override
DTSTART:20250115T100000Z
DTEND:20250115T110000Z
RECURRENCE-ID:20250114T100000Z
END:VEVENT
BEGIN:VEVENT
UID:single
SUMMARY:Holiday
DTSTART;VALUE=DATE:20250110
END:VEVENT
END:VCALENDAR
";

    #[test]
    fn expands_series_applies_exceptions_and_keeps_orphans() {
        let mut events = events_from_ics(FEED, at(6, 0), at(31, 0)).unwrap();
        events.sort_by_key(|event| event.start_date);
        let summaries: Vec<_> = events
            .iter()
            .map(|e| (e.title.as_str(), e.start_date))
            .collect();
        assert_eq!(
            summaries,
            vec![
                ("Standup", at(6, 9)),
                ("Standup", at(7, 9)),
                ("Standup (moved)", at(8, 15)),
                ("Standup", at(9, 9)),
                ("Holiday", at(10, 0)),
                ("Standup", at(10, 9)),
                ("Orphan override", at(15, 10)),
            ]
        );
        let holiday = events.iter().find(|e| e.title == "Holiday").unwrap();
        assert_eq!(holiday.is_all_day, Some(true));
        assert_eq!(holiday.end_date, Some(at(11, 0)));
    }

    #[test]
    fn filters_by_window() {
        let events = events_from_ics(FEED, at(9, 0), at(9, 23)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start_date, at(9, 9));
    }

    #[test]
    fn this_and_future_override_shifts_later_occurrences() {
        let feed = "BEGIN:VCALENDAR
BEGIN:VEVENT
UID:s
SUMMARY:Daily
DTSTART:20250106T090000Z
DTEND:20250106T100000Z
RRULE:FREQ=DAILY;COUNT=4
END:VEVENT
BEGIN:VEVENT
UID:s
SUMMARY:Daily (later)
DTSTART:20250107T110000Z
DTEND:20250107T120000Z
RECURRENCE-ID;RANGE=THISANDFUTURE:20250107T090000Z
END:VEVENT
END:VCALENDAR
";
        let mut events = events_from_ics(feed, at(6, 0), at(31, 0)).unwrap();
        events.sort_by_key(|event| event.start_date);
        let starts: Vec<_> = events
            .iter()
            .map(|e| (e.title.as_str(), e.start_date))
            .collect();
        assert_eq!(
            starts,
            vec![
                ("Daily", at(6, 9)),
                ("Daily (later)", at(7, 11)),
                ("Daily (later)", at(8, 11)),
                ("Daily (later)", at(9, 11)),
            ]
        );
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(events_from_ics("<html>not a calendar</html>", at(1, 0), at(2, 0)).is_err());
    }
}
