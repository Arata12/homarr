//! Umami: website analytics (`umami-integration.ts`).

use std::collections::BTreeMap;
use std::future::Future;

use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Timelike, Utc,
};
use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::util::{
    from_value, get_json, input_str, input_u64, q, read_json, send, send_raw, to_json,
    unknown_operation, url,
};
use crate::error::check_status;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// Umami event type ID for custom events (type 1 = page view, type 2 = custom event).
const CUSTOM_EVENT_TYPE: i64 = 2;
const EVENT_NAME_LIMIT: u32 = 5_000;
const EVENT_PAGE_SIZE: usize = 5_000;
const EVENT_PAGE_MAX: u32 = 20;
const EVENT_SERIES_CONCURRENCY: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UmamiWebsite {
    pub id: String,
    pub name: String,
    pub domain: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct Stats {
    pageviews: f64,
    visitors: f64,
    visits: f64,
    bounces: f64,
    totaltime: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataPoint {
    pub x: String,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
struct Pageviews {
    #[serde(rename = "pageviews")]
    _pageviews: Vec<DataPoint>,
    sessions: Vec<DataPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UmamiMetricItem {
    pub x: String,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
struct RawMetric {
    x: Option<String>,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct ActiveVisitors {
    visitors: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UmamiEventSeries {
    pub event_name: String,
    pub data_points: Vec<DataPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VisitorDataPoint {
    pub timestamp: String,
    pub visitors: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UmamiVisitorStats {
    pub domain: String,
    pub website_id: String,
    pub website_name: String,
    pub total_visitors: f64,
    pub total_pageviews: f64,
    pub total_visits: f64,
    pub bounce_rate: f64,
    pub avg_duration: f64,
    pub data_points: Vec<VisitorDataPoint>,
    pub time_frame: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_count: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub start_at: i64,
    pub end_at: i64,
    pub unit: &'static str,
}

fn local_instant(naive: chrono::NaiveDateTime, latest: bool) -> DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(first, second) => {
            if latest {
                second
            } else {
                first
            }
        }
        LocalResult::None => Local.from_utc_datetime(&naive),
    }
}

fn start_of_day(date: NaiveDate) -> DateTime<Local> {
    local_instant(date.and_hms_opt(0, 0, 0).unwrap_or_default(), false)
}

fn end_of_day(date: NaiveDate) -> DateTime<Local> {
    local_instant(
        date.and_hms_milli_opt(23, 59, 59, 999).unwrap_or_default(),
        true,
    )
}

fn start_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn end_of_month(date: NaiveDate) -> NaiveDate {
    let first = start_of_month(date);
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    };
    next.and_then(|next| next.pred_opt()).unwrap_or(first)
}

/// Window bounds mirror the Umami UI presets: each "Last N" preset includes the in-progress period as one of
/// N + 1 buckets, snapped to bucket boundaries. Uses the server's local timezone like dayjs does.
pub fn compute_time_range(now: DateTime<Local>, time_frame: &str) -> TimeRange {
    let today = now.date_naive();
    let range = |start: DateTime<Local>, end: DateTime<Local>, unit| TimeRange {
        start_at: start.timestamp_millis(),
        end_at: end.timestamp_millis(),
        unit,
    };
    match time_frame {
        "today" => range(start_of_day(today), end_of_day(today), "hour"),
        "7d" => range(
            start_of_day(today - Duration::days(7)),
            end_of_day(today),
            "day",
        ),
        "30d" => range(
            start_of_day(today - Duration::days(30)),
            end_of_day(today),
            "day",
        ),
        "month" => range(
            start_of_day(start_of_month(today)),
            end_of_day(end_of_month(today)),
            "day",
        ),
        "lastMonth" => {
            let in_last_month = start_of_month(today).pred_opt().unwrap_or(today);
            range(
                start_of_day(start_of_month(in_last_month)),
                end_of_day(end_of_month(in_last_month)),
                "day",
            )
        }
        // "24h" and anything unknown.
        _ => {
            let start = now - Duration::hours(24);
            let start_hour = local_instant(
                start
                    .naive_local()
                    .with_minute(0)
                    .and_then(|t| t.with_second(0))
                    .and_then(|t| t.with_nanosecond(0))
                    .unwrap_or(start.naive_local()),
                false,
            );
            let end_hour = local_instant(
                now.naive_local()
                    .with_minute(59)
                    .and_then(|t| t.with_second(59))
                    .and_then(|t| t.with_nanosecond(999_000_000))
                    .unwrap_or(now.naive_local()),
                true,
            );
            range(start_hour, end_hour, "hour")
        }
    }
}

/// Umami self-hosted returns "YYYY-MM-DD HH:MM:SS"; Umami Cloud returns ISO 8601. Both become epoch ms so
/// timestamps from /pageviews and /events join.
pub fn parse_umami_date(date: &str) -> Option<i64> {
    let normalized = if date.contains('T') {
        date.to_string()
    } else {
        format!("{}Z", date.replace(' ', "T"))
    };
    DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

fn extract_data_array(json: &Value) -> Vec<Value> {
    match json {
        Value::Array(items) => items.clone(),
        Value::Object(map) => map
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

async fn auth_headers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<(&'static str, String)>> {
    if let Some(api_key) = ctx.secret("apiKey") {
        return Ok(vec![("x-umami-api-key", api_key.to_string())]);
    }
    let token = jwt_token(rt, ctx).await?;
    Ok(vec![("Authorization", format!("Bearer {token}"))])
}

async fn jwt_token(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
    let request = rt
        .http
        .standard()
        .post(url(ctx, "/auth/login", &[])?)
        .json(&json!({
            "username": ctx.require_secret("username")?,
            "password": ctx.require_secret("password")?,
        }));
    let response = send(ctx, request).await?;
    let auth: AuthResponse = read_json(ctx, "Umami login response", response).await?;
    Ok(auth.token)
}

async fn fetch_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    url: url::Url,
    headers: &[(&'static str, String)],
) -> HomarrResult<reqwest::Response> {
    let mut request = rt.http.standard().get(url);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    send_raw(ctx, request).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let headers = auth_headers(rt, ctx).await?;
    let response = fetch_json(rt, ctx, url(ctx, "/websites", &[])?, &headers).await?;
    check_status(ctx, response)?;
    Ok(())
}

pub async fn websites(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<UmamiWebsite>> {
    let headers = auth_headers(rt, ctx).await?;
    // Self-hosted returns the array directly, Umami Cloud wraps it in `{ data: [...] }`.
    let json: Value = get_json(
        ctx,
        &rt.http,
        "Umami websites",
        url(ctx, "/websites", &[])?,
        &headers,
    )
    .await?;
    let list = match json {
        Value::Array(_) => json,
        other => other.get("data").cloned().unwrap_or(Value::Null),
    };
    from_value(ctx, "Umami websites", list)
}

async fn website_by_id(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    headers: &[(&'static str, String)],
) -> HomarrResult<UmamiWebsite> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
    get_json(
        ctx,
        &rt.http,
        "Umami website",
        url(ctx, &format!("/websites/{encoded_id}"), &[])?,
        headers,
    )
    .await
}

async fn website_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    range: TimeRange,
    headers: &[(&'static str, String)],
) -> HomarrResult<Stats> {
    let query = [("startAt", q(range.start_at)), ("endAt", q(range.end_at))];
    let encoded_id =
        percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
    get_json(
        ctx,
        &rt.http,
        "Umami stats",
        url(ctx, &format!("/websites/{encoded_id}/stats"), &query)?,
        headers,
    )
    .await
}

async fn website_pageviews(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    range: TimeRange,
    headers: &[(&'static str, String)],
) -> HomarrResult<Pageviews> {
    let query = [
        ("startAt", q(range.start_at)),
        ("endAt", q(range.end_at)),
        ("unit", q(range.unit)),
        ("timezone", q("UTC")),
    ];
    let encoded_id =
        percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
    get_json(
        ctx,
        &rt.http,
        "Umami pageviews",
        url(ctx, &format!("/websites/{encoded_id}/pageviews"), &query)?,
        headers,
    )
    .await
}

/// Failures to load metrics are logged and yield an empty list, like the TS implementation.
async fn website_metrics(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    range: (i64, i64),
    metric_type: &str,
    limit: Option<u32>,
    headers: &[(&'static str, String)],
) -> HomarrResult<Vec<UmamiMetricItem>> {
    let query = [
        ("startAt", q(range.0)),
        ("endAt", q(range.1)),
        ("type", q(metric_type)),
        ("limit", limit.map(|limit| limit.to_string())),
    ];
    let encoded_id =
        percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
    let response = fetch_json(
        rt,
        ctx,
        url(ctx, &format!("/websites/{encoded_id}/metrics"), &query)?,
        headers,
    )
    .await?;
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            website_id,
            metric_type,
            "Failed to load website metrics"
        );
        return Ok(Vec::new());
    }
    let json: Value = read_json(ctx, "Umami metrics", response).await?;
    if json.is_array() {
        let raw: Vec<RawMetric> = from_value(ctx, "Umami metrics", json)?;
        return Ok(raw
            .into_iter()
            .map(|metric| UmamiMetricItem {
                x: metric.x.unwrap_or_default(),
                y: metric.y,
            })
            .collect());
    }
    tracing::warn!(website_id, metric_type, "Unexpected metrics response shape");
    Ok(Vec::new())
}

fn truncate_bucket(iso_date: &str, unit: &str) -> Option<String> {
    let date = DateTime::parse_from_rfc3339(iso_date)
        .ok()?
        .with_timezone(&Utc);
    let day = format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day());
    Some(match unit {
        "hour" => format!("{day} {:02}:00:00", date.hour()),
        "month" => format!("{:04}-{:02}-01 00:00:00", date.year(), date.month()),
        _ => format!("{day} 00:00:00"),
    })
}

/// Pages through raw event records (or accepts pre-aggregated `{x, y}` points). `fetch_page` returns `None`
/// when the page could not be loaded, which discards all partial data.
async fn event_time_series_with<F, Fut>(
    event_name: &str,
    unit: &str,
    mut fetch_page: F,
) -> HomarrResult<Option<Vec<DataPoint>>>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = HomarrResult<Option<Value>>>,
{
    let mut buckets: BTreeMap<String, f64> = BTreeMap::new();
    let mut previous_first_id: Option<String> = None;
    let mut truncated = false;

    let mut page = 1;
    loop {
        let Some(json) = fetch_page(page).await? else {
            return Ok(None);
        };
        let raw_array = extract_data_array(&json);
        if raw_array.is_empty() {
            break;
        }
        if page > EVENT_PAGE_MAX {
            truncated = true;
            break;
        }

        let first = &raw_array[0];
        if first.get("x").is_some() && first.get("y").is_some() {
            let points: Vec<DataPoint> =
                serde_json::from_value(Value::Array(raw_array)).map_err(|err| {
                    HomarrError::Upstream(format!("Failed to parse Umami event time series: {err}"))
                })?;
            return Ok((!points.is_empty()).then_some(points));
        }
        if first.get("createdAt").is_none() {
            return Ok(None);
        }

        let first_id = match (
            first.get("id").and_then(Value::as_str),
            first.get("createdAt").and_then(Value::as_str),
        ) {
            (Some(id), _) => Some(id.to_string()),
            (None, Some(created)) => Some(format!(
                "{}@{created}",
                first.get("sessionId").and_then(Value::as_str).unwrap_or("")
            )),
            _ => None,
        };
        if page > 1 && first_id.is_some() && first_id == previous_first_id {
            tracing::warn!(
                event_name,
                "Umami ignored event paging, stopping after the first page"
            );
            break;
        }
        previous_first_id = first_id;

        for item in &raw_array {
            if item.get("eventType").and_then(Value::as_i64) != Some(CUSTOM_EVENT_TYPE)
                || item.get("eventName").and_then(Value::as_str) != Some(event_name)
            {
                continue;
            }
            let Some(created) = item.get("createdAt").and_then(Value::as_str) else {
                continue;
            };
            if let Some(bucket) = truncate_bucket(created, unit) {
                *buckets.entry(bucket).or_insert(0.0) += 1.0;
            }
        }

        if raw_array.len() < EVENT_PAGE_SIZE {
            break;
        }
        page += 1;
    }

    if truncated {
        tracing::warn!(
            event_name,
            max_records = EVENT_PAGE_SIZE as u32 * EVENT_PAGE_MAX,
            "Event time series hit the page ceiling, counts are a lower bound"
        );
    }
    if buckets.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        buckets
            .into_iter()
            .map(|(x, y)| DataPoint { x, y })
            .collect(),
    ))
}

async fn website_event_time_series(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    range: TimeRange,
    event_name: &str,
    headers: &[(&'static str, String)],
) -> HomarrResult<Option<Vec<DataPoint>>> {
    event_time_series_with(event_name, range.unit, |page| async move {
        let query = [
            ("startAt", q(range.start_at)),
            ("endAt", q(range.end_at)),
            ("unit", q(range.unit)),
            ("timezone", q("UTC")),
            ("event", q(event_name)),
            ("eventName", q(event_name)),
            ("page", q(page)),
            ("pageSize", q(EVENT_PAGE_SIZE)),
        ];
        let encoded_id =
            percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
        let response = fetch_json(
            rt,
            ctx,
            url(ctx, &format!("/websites/{encoded_id}/events"), &query)?,
            headers,
        )
        .await?;
        if !response.status().is_success() {
            tracing::warn!(
                status = response.status().as_u16(),
                website_id,
                event_name,
                page,
                "Failed to load event time series"
            );
            return Ok(None);
        }
        Ok(Some(
            read_json::<Value>(ctx, "Umami events", response).await?,
        ))
    })
    .await
}

pub async fn visitor_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    time_frame: &str,
    event_name: Option<&str>,
) -> HomarrResult<UmamiVisitorStats> {
    let headers = auth_headers(rt, ctx).await?;
    let website = website_by_id(rt, ctx, website_id, &headers).await?;
    let range = compute_time_range(Local::now(), time_frame);

    let events_future = async {
        match event_name {
            Some(name) => Ok::<_, HomarrError>((
                Some(
                    website_metrics(
                        rt,
                        ctx,
                        &website.id,
                        (range.start_at, range.end_at),
                        "event",
                        None,
                        &headers,
                    )
                    .await?,
                ),
                website_event_time_series(rt, ctx, &website.id, range, name, &headers).await?,
            )),
            None => Ok((None, None)),
        }
    };
    let (stats, pageviews, (event_metrics, event_series)) = tokio::try_join!(
        website_stats(rt, ctx, &website.id, range, &headers),
        website_pageviews(rt, ctx, &website.id, range, &headers),
        events_future,
    )?;

    let event_count = match (event_name, &event_metrics) {
        (Some(name), Some(metrics)) => Some(
            metrics
                .iter()
                .find(|metric| metric.x == name)
                .map_or(0.0, |metric| metric.y),
        ),
        _ => None,
    };
    let by_timestamp: BTreeMap<i64, f64> = event_series
        .iter()
        .flatten()
        .filter_map(|point| parse_umami_date(&point.x).map(|ts| (ts, point.y)))
        .collect();
    let has_series = event_series.is_some();
    let data_points = pageviews
        .sessions
        .into_iter()
        .map(|point| {
            let events = has_series.then(|| {
                parse_umami_date(&point.x)
                    .and_then(|ts| by_timestamp.get(&ts).copied())
                    .unwrap_or(0.0)
            });
            VisitorDataPoint {
                timestamp: point.x,
                visitors: point.y,
                events,
            }
        })
        .collect();

    Ok(UmamiVisitorStats {
        domain: website.domain,
        website_name: website.name,
        website_id: website.id,
        total_visitors: stats.visitors,
        total_pageviews: stats.pageviews,
        total_visits: stats.visits,
        bounce_rate: if stats.visits > 0.0 {
            (stats.bounces / stats.visits * 100.0).round()
        } else {
            0.0
        },
        avg_duration: if stats.visits > 0.0 {
            (stats.totaltime / stats.visits).round()
        } else {
            0.0
        },
        data_points,
        time_frame: time_frame.to_string(),
        event_count,
    })
}

pub async fn event_names(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
) -> HomarrResult<Vec<String>> {
    let headers = auth_headers(rt, ctx).await?;
    let end_at = Utc::now().timestamp_millis();
    let start_at = (Local::now() - Duration::days(30)).timestamp_millis();
    let metrics = website_metrics(
        rt,
        ctx,
        website_id,
        (start_at, end_at),
        "event",
        Some(EVENT_NAME_LIMIT),
        &headers,
    )
    .await?;
    let names: std::collections::BTreeSet<String> = metrics
        .into_iter()
        .filter(|metric| !metric.x.is_empty())
        .map(|metric| metric.x)
        .collect();
    Ok(names.into_iter().collect())
}

pub async fn active_visitors(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
) -> HomarrResult<f64> {
    let headers = auth_headers(rt, ctx).await?;
    let encoded_id =
        percent_encoding::utf8_percent_encode(website_id, percent_encoding::NON_ALPHANUMERIC);
    let response = fetch_json(
        rt,
        ctx,
        url(ctx, &format!("/websites/{encoded_id}/active"), &[])?,
        &headers,
    )
    .await?;
    if !response.status().is_success() {
        tracing::warn!(
            status = response.status().as_u16(),
            website_id,
            "Failed to load active visitors"
        );
        return Ok(0.0);
    }
    Ok(
        read_json::<ActiveVisitors>(ctx, "Umami active visitors", response)
            .await?
            .visitors,
    )
}

pub async fn top_pages(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    time_frame: &str,
    limit: u32,
) -> HomarrResult<Vec<UmamiMetricItem>> {
    let headers = auth_headers(rt, ctx).await?;
    let range = compute_time_range(Local::now(), time_frame);
    website_metrics(
        rt,
        ctx,
        website_id,
        (range.start_at, range.end_at),
        "path",
        Some(limit),
        &headers,
    )
    .await
}

pub async fn top_referrers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    time_frame: &str,
    limit: u32,
) -> HomarrResult<Vec<UmamiMetricItem>> {
    let headers = auth_headers(rt, ctx).await?;
    let range = compute_time_range(Local::now(), time_frame);
    website_metrics(
        rt,
        ctx,
        website_id,
        (range.start_at, range.end_at),
        "referrer",
        Some(limit),
        &headers,
    )
    .await
}

pub async fn multi_event_time_series(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    website_id: &str,
    time_frame: &str,
    event_names: &[String],
) -> HomarrResult<Vec<UmamiEventSeries>> {
    let headers = auth_headers(rt, ctx).await?;
    let range = compute_time_range(Local::now(), time_frame);
    let mut results = Vec::new();
    for batch in event_names.chunks(EVENT_SERIES_CONCURRENCY) {
        let series = futures_util::future::try_join_all(batch.iter().map(|name| {
            let headers = &headers;
            async move {
                let points =
                    website_event_time_series(rt, ctx, website_id, range, name, headers).await?;
                Ok::<_, HomarrError>(UmamiEventSeries {
                    event_name: name.clone(),
                    data_points: points.unwrap_or_default(),
                })
            }
        }))
        .await?;
        results.extend(series);
    }
    Ok(results)
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    let website_id = || {
        input_str(input, "websiteId")
            .map(str::to_string)
            .ok_or_else(|| HomarrError::BadRequest("websiteId is required".into()))
    };
    let time_frame = input_str(input, "timeFrame").unwrap_or("24h").to_string();
    let limit = input_u64(input, "limit").unwrap_or(10) as u32;
    match operation {
        "getWebsites" => to_json(&websites(rt, ctx).await?),
        "getVisitorStats" => to_json(
            &visitor_stats(
                rt,
                ctx,
                &website_id()?,
                &time_frame,
                input_str(input, "eventName"),
            )
            .await?,
        ),
        "getEventNames" => to_json(&event_names(rt, ctx, &website_id()?).await?),
        "getActiveVisitors" => to_json(&active_visitors(rt, ctx, &website_id()?).await?),
        "getTopPages" => to_json(&top_pages(rt, ctx, &website_id()?, &time_frame, limit).await?),
        "getTopReferrers" => {
            to_json(&top_referrers(rt, ctx, &website_id()?, &time_frame, limit).await?)
        }
        "getMultiEventTimeSeries" => {
            let names: Vec<String> = input
                .get("eventNames")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            to_json(&multi_event_time_series(rt, ctx, &website_id()?, &time_frame, &names).await?)
        }
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn raw_event(index: usize, name: &str, hour: u32) -> Value {
        json!({
            "id": format!("event-{index}"),
            "websiteId": "site-1",
            "sessionId": format!("session-{index}"),
            "eventType": 2,
            "eventName": name,
            "createdAt": format!("2026-08-09T{hour:02}:30:00.000Z"),
        })
    }

    fn sum(points: &[DataPoint]) -> f64 {
        points.iter().map(|point| point.y).sum()
    }

    #[tokio::test]
    async fn pages_through_raw_events_and_aggregates() {
        let first: Vec<Value> = (0..EVENT_PAGE_SIZE)
            .map(|i| raw_event(i, "signup", 10))
            .collect();
        let second: Vec<Value> = (0..5)
            .map(|i| raw_event(EVENT_PAGE_SIZE + i, "signup", 11))
            .collect();
        let requested = RefCell::new(Vec::new());
        let result = event_time_series_with("signup", "hour", |page| {
            requested.borrow_mut().push(page);
            let body = if page == 1 {
                first.clone()
            } else {
                second.clone()
            };
            async move { Ok(Some(json!({ "data": body }))) }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*requested.borrow(), vec![1, 2]);
        assert_eq!(sum(&result), (EVENT_PAGE_SIZE + 5) as f64);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].x, "2026-08-09 10:00:00");
    }

    #[tokio::test]
    async fn accepts_pre_aggregated_points() {
        let result = event_time_series_with("signup", "hour", |_| async {
            Ok(Some(
                json!([{"x": "2026-08-09 10:00:00", "y": 4}, {"x": "2026-08-09 11:00:00", "y": 7}]),
            ))
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].y, 7.0);
    }

    #[tokio::test]
    async fn stops_when_the_instance_ignores_paging() {
        let page: Vec<Value> = (0..EVENT_PAGE_SIZE)
            .map(|i| raw_event(i, "signup", 10))
            .collect();
        let calls = RefCell::new(0);
        let result = event_time_series_with("signup", "hour", |_| {
            *calls.borrow_mut() += 1;
            let body = page.clone();
            async move { Ok(Some(json!({ "data": body }))) }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*calls.borrow(), 2);
        assert_eq!(sum(&result), EVENT_PAGE_SIZE as f64);
    }

    #[tokio::test]
    async fn discards_partial_data_when_a_page_fails() {
        let page: Vec<Value> = (0..EVENT_PAGE_SIZE)
            .map(|i| raw_event(i, "signup", 10))
            .collect();
        let result = event_time_series_with("signup", "hour", |number| {
            let body = page.clone();
            async move {
                if number == 2 {
                    Ok(None)
                } else {
                    Ok(Some(json!({ "data": body })))
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn probes_beyond_the_ceiling_without_counting_the_sentinel_page() {
        let calls = RefCell::new(0);
        let result = event_time_series_with("signup", "hour", |number| {
            *calls.borrow_mut() += 1;
            let body: Vec<Value> = (0..EVENT_PAGE_SIZE)
                .map(|i| raw_event(number as usize * EVENT_PAGE_SIZE + i, "signup", 10))
                .collect();
            async move { Ok(Some(json!({ "data": body }))) }
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*calls.borrow(), 21);
        assert_eq!(sum(&result), (20 * EVENT_PAGE_SIZE) as f64);
    }

    #[test]
    fn parses_both_umami_date_formats() {
        assert_eq!(
            parse_umami_date("2026-08-09 10:00:00"),
            parse_umami_date("2026-08-09T10:00:00.000Z")
        );
        assert_eq!(parse_umami_date("garbage"), None);
    }

    #[test]
    fn time_ranges_follow_ui_presets() {
        let now = Local.with_ymd_and_hms(2026, 3, 15, 13, 45, 10).unwrap();
        let seven = compute_time_range(now, "7d");
        assert_eq!(seven.unit, "day");
        assert_eq!(
            Local
                .timestamp_millis_opt(seven.start_at)
                .unwrap()
                .date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 8).unwrap()
        );
        let last_month = compute_time_range(now, "lastMonth");
        let start = Local.timestamp_millis_opt(last_month.start_at).unwrap();
        let end = Local.timestamp_millis_opt(last_month.end_at).unwrap();
        assert_eq!((start.month(), start.day()), (2, 1));
        assert_eq!((end.month(), end.day()), (2, 28));
        let day = compute_time_range(now, "24h");
        assert_eq!(day.unit, "hour");
        let end_hour = Local.timestamp_millis_opt(day.end_at).unwrap();
        assert_eq!((end_hour.hour(), end_hour.minute()), (13, 59));
    }

    #[test]
    fn metric_without_label_becomes_empty_string() {
        let raw: Vec<RawMetric> = serde_json::from_value(json!([{"x": null, "y": 3}])).unwrap();
        assert_eq!(raw[0].x.clone().unwrap_or_default(), "");
    }
}
