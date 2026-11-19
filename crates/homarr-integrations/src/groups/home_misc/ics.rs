//! Minimal iCalendar (RFC 5545) support shared by the iCal and Nextcloud integrations: content line parsing,
//! VEVENT extraction and recurrence expansion (via the `rrule` crate). Replaces `ical.js`.
//!
//! Time zones are resolved from IANA `TZID`s; floating times are treated as UTC (like a server running in UTC).

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use homarr_common::{HomarrError, HomarrResult};
use rrule::RRuleSet;

/// Caps the walk so a high frequency rule on an untrusted feed can't hang the request.
pub const MAX_OCCURRENCES: u16 = u16::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props
            .iter()
            .find(|prop| prop.name.eq_ignore_ascii_case(name))
    }

    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> {
        self.props
            .iter()
            .filter(move |prop| prop.name.eq_ignore_ascii_case(name))
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Component> {
        self.children
            .iter()
            .filter(move |child| child.name.eq_ignore_ascii_case(name))
    }
}

/// Unfolds continuation lines (a leading space or tab continues the previous line).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push_str(&raw[1..]);
            }
        } else if !raw.is_empty() {
            lines.push(raw.to_string());
        }
    }
    lines
}

/// Splits `NAME;PARAM=a;PARAM2="b;c":value` honouring quoted parameter values.
fn parse_line(line: &str) -> Option<Prop> {
    let mut in_quotes = false;
    let mut colon = None;
    for (index, character) in line.char_indices() {
        match character {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(index);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);

    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in head.chars() {
        match character {
            '"' => {
                quoted = !quoted;
            }
            ';' if !quoted => parts.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    parts.push(current);
    let name = parts.remove(0).to_uppercase();
    let params = parts
        .into_iter()
        .filter_map(|part| {
            part.split_once('=')
                .map(|(key, value)| (key.to_uppercase(), value.to_string()))
        })
        .collect();
    Some(Prop {
        name,
        params,
        value: value.to_string(),
    })
}

/// Parses the content lines into the component tree. Fails when no `VCALENDAR` is present.
pub fn parse(text: &str) -> HomarrResult<Component> {
    let mut stack: Vec<Component> = vec![Component {
        name: "ROOT".into(),
        ..Default::default()
    }];
    for line in unfold(text) {
        let Some(prop) = parse_line(&line) else {
            continue;
        };
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component {
                name: prop.value.to_uppercase(),
                ..Default::default()
            }),
            "END" => {
                if stack.len() > 1 {
                    if let Some(done) = stack.pop() {
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(done);
                        }
                    }
                }
            }
            _ => {
                if let Some(top) = stack.last_mut() {
                    top.props.push(prop);
                }
            }
        }
    }
    // Unterminated components are closed implicitly.
    while stack.len() > 1 {
        if let Some(done) = stack.pop() {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(done);
            }
        }
    }
    let root = stack.pop().unwrap_or_default();
    root.children
        .into_iter()
        .find(|child| child.name == "VCALENDAR")
        .ok_or_else(|| HomarrError::Upstream("Failed to parse calendar: no VCALENDAR found".into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcsTime {
    Date(NaiveDate),
    /// Floating time (no zone), treated as UTC.
    Floating(NaiveDateTime),
    Utc(DateTime<Utc>),
    Zoned(Tz, NaiveDateTime),
}

impl IcsTime {
    pub fn is_date(&self) -> bool {
        matches!(self, Self::Date(_))
    }

    /// The instant. `DATE` values map to UTC midnight (`local_dates = false`) or to local midnight.
    pub fn to_utc(self, local_dates: bool) -> DateTime<Utc> {
        match self {
            Self::Date(date) => {
                let midnight = date.and_time(NaiveTime::MIN);
                if local_dates {
                    chrono::Local
                        .from_local_datetime(&midnight)
                        .earliest()
                        .map_or_else(|| midnight.and_utc(), |local| local.with_timezone(&Utc))
                } else {
                    midnight.and_utc()
                }
            }
            Self::Floating(naive) => naive.and_utc(),
            Self::Utc(utc) => utc,
            Self::Zoned(tz, naive) => tz
                .from_local_datetime(&naive)
                .earliest()
                .or_else(|| {
                    tz.from_local_datetime(&(naive + Duration::hours(1)))
                        .earliest()
                })
                .map_or_else(|| naive.and_utc(), |zoned| zoned.with_timezone(&Utc)),
        }
    }
}

pub fn parse_time(prop: &Prop) -> Option<IcsTime> {
    let value = prop.value.trim();
    if prop
        .param("VALUE")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE"))
        || (value.len() == 8 && !value.contains('T'))
    {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .map(IcsTime::Date);
    }
    if let Some(stripped) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
        return Some(IcsTime::Utc(naive.and_utc()));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    match prop.param("TZID").and_then(|tzid| tzid.parse::<Tz>().ok()) {
        Some(tz) => Some(IcsTime::Zoned(tz, naive)),
        None => Some(IcsTime::Floating(naive)),
    }
}

/// `P1DT2H30M`-style durations (RFC 5545 section 3.3.6).
pub fn parse_duration(value: &str) -> Option<Duration> {
    let mut rest = value.trim();
    let negative = rest.starts_with('-');
    rest = rest.trim_start_matches(['+', '-']).strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut in_time = false;
    for character in rest.chars() {
        match character {
            'T' => in_time = true,
            '0'..='9' => number.push(character),
            unit => {
                let amount: i64 = number.parse().ok()?;
                number.clear();
                let delta = match (unit, in_time) {
                    ('W', _) => Duration::try_weeks(amount)?,
                    ('D', _) => Duration::try_days(amount)?,
                    ('H', true) => Duration::try_hours(amount)?,
                    ('M', true) => Duration::try_minutes(amount)?,
                    ('S', true) => Duration::try_seconds(amount)?,
                    _ => return None,
                };
                total = total.checked_add(&delta)?;
            }
        }
    }
    if negative {
        Duration::zero().checked_sub(&total)
    } else {
        Some(total)
    }
}

#[derive(Debug, Clone)]
pub struct VEvent {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub color: Option<String>,
    pub start: IcsTime,
    pub end: Option<IcsTime>,
    pub duration: Option<Duration>,
    pub rrule: Option<String>,
    pub exdates: Vec<IcsTime>,
    pub rdates: Vec<IcsTime>,
    pub recurrence_id: Option<IcsTime>,
    pub range_this_and_future: bool,
}

/// Unescapes TEXT values (`\n`, `\,`, `\;`, `\\`).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

impl VEvent {
    /// `None` when the component has no usable `DTSTART`.
    pub fn from_component(component: &Component) -> Option<Self> {
        let text = |name: &str| {
            component
                .prop(name)
                .map(|prop| unescape(&prop.value))
                .unwrap_or_default()
        };
        let start = component.prop("DTSTART").and_then(parse_time)?;
        let times = |name: &str| -> Vec<IcsTime> {
            component
                .props_named(name)
                .flat_map(|prop| {
                    prop.value
                        .split(',')
                        .filter_map(|part| {
                            parse_time(&Prop {
                                name: prop.name.clone(),
                                params: prop.params.clone(),
                                value: part.to_string(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let recurrence = component.prop("RECURRENCE-ID");
        Some(Self {
            uid: text("UID"),
            summary: text("SUMMARY"),
            description: text("DESCRIPTION"),
            location: text("LOCATION"),
            color: component
                .prop("COLOR")
                .map(|prop| prop.value.clone())
                .filter(|c| !c.is_empty()),
            start,
            end: component.prop("DTEND").and_then(parse_time),
            duration: component
                .prop("DURATION")
                .and_then(|prop| parse_duration(&prop.value)),
            rrule: component.prop("RRULE").map(|prop| prop.value.clone()),
            exdates: times("EXDATE"),
            rdates: times("RDATE"),
            recurrence_id: recurrence.and_then(parse_time),
            range_this_and_future: recurrence
                .and_then(|prop| prop.param("RANGE"))
                .is_some_and(|range| range.eq_ignore_ascii_case("THISANDFUTURE")),
        })
    }

    pub fn is_recurring(&self) -> bool {
        self.rrule.is_some() || !self.rdates.is_empty()
    }

    pub fn is_recurrence_exception(&self) -> bool {
        self.recurrence_id.is_some()
    }

    /// Event length like `ICAL.Event.duration`: DURATION, else DTEND - DTSTART, else one day for `DATE` starts.
    pub fn length(&self, local_dates: bool) -> Duration {
        if let Some(duration) = self.duration {
            return duration;
        }
        if let Some(end) = self.end {
            return end.to_utc(local_dates) - self.start.to_utc(local_dates);
        }
        if self.start.is_date() {
            Duration::days(1)
        } else {
            Duration::zero()
        }
    }
}

fn format_utc(time: DateTime<Utc>) -> String {
    time.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Rewrites `UNTIL` as a UTC date-time, as required when DTSTART carries a zone (RFC 5545 section 3.3.10).
fn normalize_rrule(rrule: &str, start: &IcsTime, local_dates: bool) -> String {
    rrule
        .split(';')
        .map(|part| match part.split_once('=') {
            Some((key, value)) if key.eq_ignore_ascii_case("UNTIL") => {
                let prop = Prop {
                    name: "UNTIL".into(),
                    params: match start {
                        IcsTime::Zoned(tz, _) => vec![("TZID".to_string(), tz.name().to_string())],
                        _ => Vec::new(),
                    },
                    value: value.to_string(),
                };
                match parse_time(&prop) {
                    Some(IcsTime::Date(date)) => {
                        let end_of_day = date.and_hms_opt(23, 59, 59).unwrap_or_default();
                        format!("UNTIL={}", format_utc(end_of_day.and_utc()))
                    }
                    Some(time) => format!("UNTIL={}", format_utc(time.to_utc(local_dates))),
                    None => part.to_string(),
                }
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn dtstart_line(start: &IcsTime, local_dates: bool) -> String {
    match start {
        IcsTime::Zoned(tz, naive) => format!(
            "DTSTART;TZID={}:{}",
            tz.name(),
            naive.format("%Y%m%dT%H%M%S")
        ),
        other => format!("DTSTART:{}", format_utc(other.to_utc(local_dates))),
    }
}

/// Start instants of every occurrence of a recurring event (exceptions via `EXDATE` removed, `RDATE`s added),
/// sorted. `window` limits how far occurrences are generated.
pub fn occurrences(
    event: &VEvent,
    window: (DateTime<Utc>, DateTime<Utc>),
    local_dates: bool,
) -> HomarrResult<(Vec<DateTime<Utc>>, bool)> {
    let mut starts: Vec<DateTime<Utc>> = Vec::new();
    let mut limited = false;
    if let Some(rrule) = &event.rrule {
        let source = format!(
            "{}\nRRULE:{}",
            dtstart_line(&event.start, local_dates),
            normalize_rrule(rrule, &event.start, local_dates)
        );
        let set: RRuleSet = source.parse().map_err(|err| {
            HomarrError::Upstream(format!("Invalid recurrence rule '{rrule}': {err}"))
        })?;
        let before = window.1.with_timezone(&rrule::Tz::UTC);
        let after = (window.0 - Duration::days(1)).with_timezone(&rrule::Tz::UTC);
        let result = set.after(after).before(before).all(MAX_OCCURRENCES);
        limited = result.limited;
        starts.extend(
            result
                .dates
                .into_iter()
                .map(|date| date.with_timezone(&Utc)),
        );
    } else {
        starts.push(event.start.to_utc(local_dates));
    }
    starts.extend(event.rdates.iter().map(|time| time.to_utc(local_dates)));
    let excluded: Vec<DateTime<Utc>> = event
        .exdates
        .iter()
        .map(|time| time.to_utc(local_dates))
        .collect();
    starts.retain(|start| !excluded.contains(start));
    starts.sort();
    starts.dedup();
    Ok((starts, limited))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Stand\\, up\r\nDESCRIPTION:line1\\nli\r\n ne2\r\nDTSTART;TZID=Europe/Berlin:20250106T090000\r\nDTEND;TZID=Europe/Berlin:20250106T093000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE;TZID=Europe/Berlin:20250107T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn events(text: &str) -> Vec<VEvent> {
        parse(text)
            .unwrap()
            .children_named("VEVENT")
            .filter_map(VEvent::from_component)
            .collect()
    }

    #[test]
    fn parses_folded_and_escaped_text() {
        let events = events(SAMPLE);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Stand, up");
        assert_eq!(events[0].description, "line1\nline2");
        assert_eq!(events[0].length(false), Duration::minutes(30));
    }

    #[test]
    fn rejects_text_without_a_calendar() {
        assert!(parse("hello world").is_err());
    }

    #[test]
    fn expands_rrule_respecting_zone_and_exdate() {
        let event = &events(SAMPLE)[0];
        let window = (
            Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap(),
        );
        let (starts, limited) = occurrences(event, window, false).unwrap();
        assert!(!limited);
        // 06 and 08 January remain (the 07th is excluded); 09:00 Berlin (CET) is 08:00 UTC.
        assert_eq!(
            starts,
            vec![
                Utc.with_ymd_and_hms(2025, 1, 6, 8, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2025, 1, 8, 8, 0, 0).unwrap()
            ]
        );
    }

    #[test]
    fn all_day_events_default_to_one_day_and_utc_midnight() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:b\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20250301\nEND:VEVENT\nEND:VCALENDAR\n";
        let event = &events(text)[0];
        assert!(event.start.is_date());
        assert_eq!(event.length(false), Duration::days(1));
        assert_eq!(
            event.start.to_utc(false),
            Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn until_is_normalized_for_zoned_starts() {
        let start = IcsTime::Zoned(
            chrono_tz::Europe::Berlin,
            NaiveDate::from_ymd_opt(2025, 1, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
        );
        assert_eq!(
            normalize_rrule("FREQ=DAILY;UNTIL=20250110", &start, false),
            "FREQ=DAILY;UNTIL=20250110T235959Z"
        );
        assert_eq!(
            parse_duration("-P1DT2H"),
            Some(-(Duration::days(1) + Duration::hours(2)))
        );
        assert_eq!(parse_duration("P999999999999D"), None);
    }
}
