//! Wazuh: server API + indexer client (`wazuh-integration.ts`, `wazuh-errors.ts`).
//!
//! Credential alternatives:
//! - username + password: the integration URL is the Wazuh server API (default port 55000).
//! - username + password + indexer URL/credentials: server API plus the indexer for alerts.
//! - indexer username + password only: the integration URL is the Wazuh indexer (default port 9200).

mod queries;
mod types;

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use homarr_common::{HomarrError, HomarrResult};
use reqwest::{Method, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use self::queries::{
    create_alert_dashboard_url, format_alert_notification_body, map_alert_hit,
    normalize_agent_status, normalize_version, resolve_timeline_interval_ms,
    severity_range_aggregation, time_range_ms, to_severity_counts, total_hits, ALERT_SOURCE_FIELDS,
};
use self::types::*;
use super::util::{basic_auth, input_str, input_u64, to_json, unknown_operation};
use crate::capabilities::notifications::Notification;
use crate::error::{classify_request_error, RequestFailure};
use crate::http_auth::ResolvedHttpAuth;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The Wazuh API issues 900 second tokens by default; refresh well before that.
const TOKEN_TTL: Duration = Duration::from_secs(600);
/// Agent snapshots in wazuh-monitoring-* are written as one batch that spans a few milliseconds.
const MONITORING_SNAPSHOT_WINDOW_MS: i64 = 5 * 60 * 1000;
/// Upper bound for agent lists; larger fleets should use the Wazuh dashboard.
const MAX_AGENTS: u64 = 1000;
/// Matches Wazuh's default email_alert_level: the notifications widget lists what Wazuh would send as email.
const NOTIFICATION_MIN_LEVEL: i64 = 12;
const NOTIFICATION_WINDOW_HOURS: u64 = 24;
const NOTIFICATION_LIMIT: u64 = 25;

#[derive(Debug, Serialize, Deserialize)]
struct WazuhSession {
    token: String,
}

/// Safe, user presentable failure (`WazuhRequestError`). Never carries credentials or upstream payloads.
#[derive(Debug)]
struct WazuhError {
    target: Option<&'static str>,
    reason: &'static str,
    status: Option<u16>,
    detail: Option<String>,
}

impl WazuhError {
    fn new(target: Option<&'static str>, reason: &'static str) -> Self {
        Self {
            target,
            reason,
            status: None,
            detail: None,
        }
    }

    fn from_response(target: &'static str, response: &Response) -> Self {
        let status = response.status().as_u16();
        let reason = if status == 401 || status == 403 {
            "unauthorized"
        } else {
            "status"
        };
        Self {
            target: Some(target),
            reason,
            status: Some(status),
            detail: None,
        }
    }

    fn from_request(target: &'static str, err: &reqwest::Error) -> Self {
        let reason = match classify_request_error(err) {
            RequestFailure::Certificate => "certificate",
            RequestFailure::Timeout => "timeout",
            RequestFailure::Connection | RequestFailure::Redirect | RequestFailure::Other => {
                "unreachable"
            }
        };
        Self {
            target: Some(target),
            reason,
            status: None,
            detail: None,
        }
    }
}

impl From<WazuhError> for HomarrError {
    fn from(err: WazuhError) -> Self {
        let target = err.target.unwrap_or("integration");
        let hint = match err.reason {
            "unauthorized" => "the credentials were rejected".to_string(),
            "certificate" => {
                "the TLS certificate is not trusted; add it to the trusted certificates".to_string()
            }
            "certificateHostname" => "the TLS certificate was not issued for this host".to_string(),
            "certificateExpired" => "the TLS certificate is expired or not valid yet".to_string(),
            "unreachable" => "the service could not be reached".to_string(),
            "timeout" => "the request timed out".to_string(),
            "status" => format!("unexpected HTTP status {}", err.status.unwrap_or_default()),
            "invalidResponse" => "the response could not be understood".to_string(),
            "indexerRequired" => {
                "this data needs the Wazuh indexer credentials (and URL) to be configured"
                    .to_string()
            }
            other => other.to_string(),
        };
        let detail = err
            .detail
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        HomarrError::Upstream(format!("Wazuh {target} request failed: {hint}{detail}"))
    }
}

type WResult<T> = Result<T, WazuhError>;

#[derive(Debug, Clone)]
struct Endpoint {
    url: String,
    username: String,
    password: String,
}

struct Endpoints {
    api: Option<Endpoint>,
    indexer: Option<Endpoint>,
}

fn endpoints(ctx: &IntegrationContext) -> Endpoints {
    let has = |key: &str| ctx.secret(key).is_some();
    let get = |key: &str| ctx.secret(key).unwrap_or_default().to_string();
    let has_api = has("username") && has("password");
    let has_indexer_credentials = has("wazuhIndexerUsername") && has("wazuhIndexerPassword");

    let api = has_api.then(|| Endpoint {
        url: ctx.url.trim_end_matches('/').to_string(),
        username: get("username"),
        password: get("password"),
    });

    let mut indexer_url = None;
    if has_indexer_credentials {
        if has("wazuhIndexerUrl") {
            indexer_url = Some(get("wazuhIndexerUrl"));
        } else if !has_api {
            indexer_url = Some(ctx.url.clone());
        }
    }
    let indexer = indexer_url.map(|url| Endpoint {
        url: url.trim_end_matches('/').to_string(),
        username: get("wazuhIndexerUsername"),
        password: get("wazuhIndexerPassword"),
    });
    Endpoints { api, indexer }
}

/// The external URL when it differs from the integration URL (the Wazuh dashboard).
fn dashboard_url(ctx: &IntegrationContext) -> Option<String> {
    let external = ctx.external_url.as_deref()?.trim_end_matches('/');
    (!external.is_empty() && external != ctx.url.trim_end_matches('/'))
        .then(|| external.to_string())
}

async fn request(
    rt: &IntegrationRuntime,
    target: &'static str,
    method: Method,
    url: &str,
    headers: &[(&str, String)],
    body: Option<String>,
) -> WResult<Response> {
    let mut builder = rt
        .http
        .standard()
        .request(method, url)
        .timeout(REQUEST_TIMEOUT);
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    if let Some(body) = body {
        builder = builder.body(body);
    }
    builder
        .send()
        .await
        .map_err(|err| WazuhError::from_request(target, &err))
}

async fn json_body<T: DeserializeOwned>(target: &'static str, response: Response) -> WResult<T> {
    let text = response
        .text()
        .await
        .map_err(|err| WazuhError::from_request(target, &err))?;
    serde_json::from_str(&text).map_err(|_| WazuhError::new(Some(target), "invalidResponse"))
}

async fn authenticate_api(rt: &IntegrationRuntime, api: &Endpoint) -> WResult<String> {
    let response = request(
        rt,
        "api",
        Method::POST,
        &format!("{}/security/user/authenticate?raw=true", api.url),
        &[("Authorization", basic_auth(&api.username, &api.password))],
        None,
    )
    .await?;
    if !response.status().is_success() {
        return Err(WazuhError::from_response("api", &response));
    }
    let token = response
        .text()
        .await
        .map_err(|err| WazuhError::from_request("api", &err))?
        .trim()
        .to_string();
    if token.is_empty() || token.starts_with('{') {
        return Err(WazuhError::new(Some("api"), "invalidResponse"));
    }
    Ok(token)
}

async fn api_token(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    api: &Endpoint,
) -> WResult<String> {
    if let Some(session) = rt.sessions.get::<WazuhSession>(ctx) {
        return Ok(session.token);
    }
    let token = authenticate_api(rt, api).await?;
    rt.sessions.set(
        ctx,
        &WazuhSession {
            token: token.clone(),
        },
        Some(TOKEN_TTL),
    );
    Ok(token)
}

/// Credentials for custom widget HTTP requests (port of `getHttpAuthenticationAsync`): a bearer token for the
/// Wazuh API, otherwise basic auth for the indexer.
pub async fn http_auth(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ResolvedHttpAuth> {
    let Endpoints { api, indexer } = endpoints(ctx);
    let (header, redact_values) = if let Some(api) = api {
        let token = api_token(rt, ctx, &api).await?;
        (format!("Bearer {token}"), vec![token])
    } else if let Some(indexer) = indexer {
        let header = basic_auth(&indexer.username, &indexer.password);
        (header.clone(), vec![header, indexer.password])
    } else {
        return Err(HomarrError::BadRequest(
            "Wazuh integration has no usable credentials".to_string(),
        ));
    };
    Ok(ResolvedHttpAuth {
        headers: [("Authorization".to_string(), header)].into(),
        redact_values,
        ..Default::default()
    })
}

async fn api_get<T: DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, String)],
) -> WResult<T> {
    let api = endpoints(ctx)
        .api
        .ok_or_else(|| WazuhError::new(Some("api"), "unauthorized"))?;
    let mut url = url::Url::parse(&format!("{}{path}", api.url))
        .map_err(|_| WazuhError::new(Some("api"), "invalidResponse"))?;
    for (key, value) in query {
        url.query_pairs_mut().append_pair(key, value);
    }
    let attempt = |token: String| {
        let url = url.to_string();
        async move {
            request(
                rt,
                "api",
                Method::GET,
                &url,
                &[("Authorization", format!("Bearer {token}"))],
                None,
            )
            .await
        }
    };
    let mut response = attempt(api_token(rt, ctx, &api).await?).await?;
    if response.status().as_u16() == 401 {
        rt.sessions.clear(ctx);
        response = attempt(api_token(rt, ctx, &api).await?).await?;
    }
    if !response.status().is_success() {
        return Err(WazuhError::from_response("api", &response));
    }
    let body: ApiResponse<T> = json_body("api", response).await?;
    Ok(body.data)
}

async fn indexer_search<S: DeserializeOwned, A: DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    index_pattern: &str,
    query: Value,
) -> WResult<IndexerSearchResponse<S, A>> {
    let indexer = endpoints(ctx)
        .indexer
        .ok_or_else(|| WazuhError::new(Some("indexer"), "indexerRequired"))?;
    let response = request(
        rt,
        "indexer",
        Method::POST,
        &format!(
            "{}/{index_pattern}/_search?ignore_unavailable=true&allow_no_indices=true",
            indexer.url
        ),
        &[
            (
                "Authorization",
                basic_auth(&indexer.username, &indexer.password),
            ),
            ("Content-Type", "application/json".to_string()),
        ],
        Some(query.to_string()),
    )
    .await?;
    if !response.status().is_success() {
        return Err(WazuhError::from_response("indexer", &response));
    }
    json_body("indexer", response).await
}

async fn indexer_get<T: DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> WResult<T> {
    let indexer = endpoints(ctx)
        .indexer
        .ok_or_else(|| WazuhError::new(Some("indexer"), "indexerRequired"))?;
    let response = request(
        rt,
        "indexer",
        Method::GET,
        &format!("{}{path}", indexer.url),
        &[(
            "Authorization",
            basic_auth(&indexer.username, &indexer.password),
        )],
        None,
    )
    .await?;
    if !response.status().is_success() {
        return Err(WazuhError::from_response("indexer", &response));
    }
    json_body("indexer", response).await
}

fn iso(millis: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn range_ms(range: &str) -> HomarrResult<i64> {
    time_range_ms(range)
        .ok_or_else(|| HomarrError::BadRequest(format!("Unknown Wazuh time range '{range}'")))
}

// ---- test connection ---------------------------------------------------------------------------

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let Endpoints { api, indexer } = endpoints(ctx);
    if api.is_none() && indexer.is_none() {
        return Err(WazuhError::new(None, "unauthorized").into());
    }
    if let Some(api) = &api {
        authenticate_api(rt, api).await?;
    }
    if indexer.is_some() {
        // A secondary indexer URL may live on another host; it is checked with the same trust settings.
        indexer_search::<Value, Value>(rt, ctx, "wazuh-alerts-*", json!({ "size": 0 })).await?;
    }
    Ok(())
}

// ---- agents ------------------------------------------------------------------------------------

pub async fn agents_overview(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<AgentsOverview> {
    if endpoints(ctx).api.is_some() {
        Ok(agents_overview_from_api(rt, ctx).await?)
    } else {
        Ok(agents_overview_from_indexer(rt, ctx).await?)
    }
}

fn map_api_agent(agent: ApiAgent) -> Agent {
    let os = agent.os.unwrap_or_default();
    Agent {
        name: agent.name.unwrap_or_else(|| agent.id.clone()),
        id: agent.id,
        status: normalize_agent_status(agent.status.as_deref()).to_string(),
        ip: agent.ip,
        os_name: os.name,
        os_version: os.version,
        os_platform: os.platform.map(|platform| platform.to_lowercase()),
        version: normalize_version(agent.version.as_deref()),
        last_keep_alive: agent.last_keep_alive,
        groups: agent.group.map(StringOrList::into_vec).unwrap_or_default(),
    }
}

async fn agents_overview_from_api(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> WResult<AgentsOverview> {
    let agent_query = [
        (
            "select",
            "id,name,ip,status,version,lastKeepAlive,group,os.name,os.version,os.platform"
                .to_string(),
        ),
        ("sort", "+name".to_string()),
        ("limit", MAX_AGENTS.to_string()),
    ];
    let (summary, agents) = tokio::try_join!(
        api_get::<ApiAgentsSummaryStatus>(rt, ctx, "/agents/summary/status", &[]),
        api_get::<ApiItems<ApiAgent>>(rt, ctx, "/agents", &agent_query),
    )?;
    let count = |key: &str| summary.connection.get(key).copied().unwrap_or(0);
    Ok(AgentsOverview {
        source: DataSource::Api,
        as_of: iso(now_ms()),
        counts: AgentCounts {
            active: count("active"),
            disconnected: count("disconnected"),
            never_connected: count("never_connected"),
            pending: count("pending"),
            total: count("total"),
        },
        // Agent 000 is the manager itself; the Wazuh dashboard hides it from the agent list as well.
        agents: agents
            .affected_items
            .into_iter()
            .filter(|agent| agent.id != "000")
            .map(map_api_agent)
            .collect(),
    })
}

#[derive(Debug, Deserialize)]
struct LatestAgg {
    latest: Metric,
}

async fn agents_overview_from_indexer(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> WResult<AgentsOverview> {
    let latest = indexer_search::<Value, LatestAgg>(
        rt,
        ctx,
        "wazuh-monitoring-*",
        json!({ "size": 0, "aggs": { "latest": { "max": { "field": "timestamp" } } } }),
    )
    .await?;
    let Some(latest_timestamp) = latest
        .aggregations
        .and_then(|aggs| aggs.latest.value)
        .filter(|v| *v != 0.0)
    else {
        return Ok(AgentsOverview {
            source: DataSource::Indexer,
            as_of: iso(now_ms()),
            counts: AgentCounts::default(),
            agents: Vec::new(),
        });
    };
    let latest_ms = latest_timestamp as i64;

    let snapshot = indexer_search::<MonitoringSource, Value>(
        rt,
        ctx,
        "wazuh-monitoring-*",
        json!({
            "size": MAX_AGENTS,
            "sort": [{ "timestamp": { "order": "desc" } }],
            "_source": ["id", "name", "ip", "status", "version", "lastKeepAlive", "timestamp", "group", "os"],
            "query": { "range": { "timestamp": {
                "gte": iso(latest_ms - MONITORING_SNAPSHOT_WINDOW_MS),
                "lte": iso(latest_ms),
            } } }
        }),
    )
    .await?;
    Ok(build_indexer_overview(snapshot.hits.hits, latest_ms))
}

fn build_indexer_overview(
    hits: Vec<IndexerHit<MonitoringSource>>,
    latest_ms: i64,
) -> AgentsOverview {
    // Keep the newest document per agent in case the window overlaps two snapshots.
    let mut agents: BTreeMap<String, Agent> = BTreeMap::new();
    for hit in hits {
        let source = hit.source;
        let Some(id) = source.id.filter(|id| !id.is_empty() && id != "000") else {
            continue;
        };
        if agents.contains_key(&id) {
            continue;
        }
        let os = source.os.unwrap_or_default();
        agents.insert(
            id.clone(),
            Agent {
                name: source.name.unwrap_or_else(|| id.clone()),
                id,
                status: normalize_agent_status(source.status.as_deref()).to_string(),
                ip: source.ip,
                os_name: os.name,
                os_version: os.version,
                os_platform: os.platform.map(|platform| platform.to_lowercase()),
                version: normalize_version(source.version.as_deref()),
                last_keep_alive: source.last_keep_alive,
                groups: source.group.map(StringOrList::into_vec).unwrap_or_default(),
            },
        );
    }
    let mut counts = AgentCounts::default();
    for agent in agents.values() {
        counts.total += 1;
        match agent.status.as_str() {
            "active" => counts.active += 1,
            "disconnected" => counts.disconnected += 1,
            "never_connected" => counts.never_connected += 1,
            "pending" => counts.pending += 1,
            _ => {}
        }
    }
    let mut list: Vec<Agent> = agents.into_values().collect();
    list.sort_by_key(|agent| agent.name.to_lowercase());
    AgentsOverview {
        source: DataSource::Indexer,
        as_of: iso(latest_ms),
        counts,
        agents: list,
    }
}

// ---- alerts ------------------------------------------------------------------------------------

/// Alerts (indexer only, the server API has no alert search).
pub async fn recent_alerts(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    min_level: i64,
    limit: u64,
    hours: u64,
) -> HomarrResult<Vec<Alert>> {
    let response = indexer_search::<AlertSource, Value>(
        rt,
        ctx,
        "wazuh-alerts-*",
        json!({
            "size": limit,
            "sort": [{ "timestamp": { "order": "desc" } }],
            "_source": ALERT_SOURCE_FIELDS,
            "query": { "bool": { "filter": [
                { "range": { "rule.level": { "gte": min_level } } },
                { "range": { "timestamp": { "gte": format!("now-{hours}h") } } },
            ] } }
        }),
    )
    .await?;
    Ok(response.hits.hits.into_iter().map(map_alert_hit).collect())
}

/// High and critical alerts of the last 24 hours for the notifications widget. Needs the indexer, like all alerts.
pub async fn notifications(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Notification>> {
    let alerts = recent_alerts(
        rt,
        ctx,
        NOTIFICATION_MIN_LEVEL,
        NOTIFICATION_LIMIT,
        NOTIFICATION_WINDOW_HOURS,
    )
    .await?;
    let dashboard = dashboard_url(ctx);
    Ok(alerts
        .into_iter()
        .map(|alert| Notification {
            time: DateTime::parse_from_rfc3339(&alert.timestamp)
                .map(|time| time.with_timezone(&Utc))
                .unwrap_or_default(),
            title: if alert.description.is_empty() {
                format!("Wazuh rule {}", alert.rule_id.as_deref().unwrap_or("alert"))
            } else {
                alert.description.clone()
            },
            body: format_alert_notification_body(&alert),
            href: dashboard
                .as_deref()
                .map(|dashboard| create_alert_dashboard_url(dashboard, &alert)),
            id: alert.id,
            content_type: None,
            source: None,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct TimelineBucket {
    key: f64,
    #[serde(default)]
    doc_count: u64,
    #[serde(default)]
    severity: Buckets,
}

#[derive(Debug, Deserialize, Default)]
struct TimelineBuckets {
    #[serde(default)]
    buckets: Vec<TimelineBucket>,
}

#[derive(Debug, Deserialize)]
struct TimelineAggs {
    #[serde(default)]
    timeline: TimelineBuckets,
    #[serde(default)]
    severity: Buckets,
}

pub async fn alert_timeline(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    range: &str,
    interval: &str,
    min_level: i64,
) -> HomarrResult<AlertTimeline> {
    let duration = range_ms(range)?;
    let interval_ms = resolve_timeline_interval_ms(duration, range, interval);
    let now = now_ms();
    // Query exactly the selected window (so totals match the summary widget) but align the buckets to the
    // interval so they stay stable across refreshes. The first and last buckets can therefore be partial.
    let window_start = now - duration;
    let end = (now + interval_ms - 1).div_euclid(interval_ms) * interval_ms;
    let start = window_start.div_euclid(interval_ms) * interval_ms;

    let mut filter = vec![
        json!({ "range": { "timestamp": { "gte": window_start, "lte": now, "format": "epoch_millis" } } }),
    ];
    if min_level > 0 {
        filter.push(json!({ "range": { "rule.level": { "gte": min_level } } }));
    }
    let response = indexer_search::<Value, TimelineAggs>(
        rt,
        ctx,
        "wazuh-alerts-*",
        json!({
            "size": 0,
            "track_total_hits": true,
            "query": { "bool": { "filter": filter } },
            "aggs": {
                "timeline": {
                    "date_histogram": {
                        "field": "timestamp",
                        "fixed_interval": format!("{}s", (interval_ms as f64 / 1000.0).round() as i64),
                        "min_doc_count": 0,
                        "extended_bounds": { "min": start, "max": end - interval_ms },
                    },
                    "aggs": { "severity": severity_range_aggregation() }
                },
                "severity": severity_range_aggregation()
            }
        }),
    )
    .await?;
    let total = total_hits(&response.hits.total);
    let aggregations = response.aggregations;
    let (totals, points) = match aggregations {
        Some(aggs) => (
            to_severity_counts(Some(&aggs.severity.buckets), total),
            aggs.timeline
                .buckets
                .into_iter()
                .map(|bucket| {
                    let counts =
                        to_severity_counts(Some(&bucket.severity.buckets), bucket.doc_count);
                    TimelinePoint {
                        timestamp: bucket.key as i64,
                        low: counts.low,
                        medium: counts.medium,
                        high: counts.high,
                        critical: counts.critical,
                    }
                })
                .collect(),
        ),
        None => (to_severity_counts(None, total), Vec::new()),
    };
    Ok(AlertTimeline {
        range: range.to_string(),
        interval_ms,
        totals,
        points,
    })
}

#[derive(Debug, Deserialize)]
struct TopBucket {
    #[serde(deserialize_with = "de_key")]
    key: String,
    #[serde(default)]
    doc_count: u64,
    #[serde(default, rename = "maxLevel")]
    max_level: Metric,
    #[serde(default)]
    agents: Metric,
    #[serde(default)]
    label: Option<Buckets>,
    #[serde(default)]
    detail: Option<Buckets>,
}

#[derive(Debug, Deserialize, Default)]
struct TopTerms {
    #[serde(default)]
    buckets: Vec<TopBucket>,
    #[serde(default)]
    sum_other_doc_count: u64,
}

#[derive(Debug, Deserialize)]
struct TopAggs {
    #[serde(default)]
    top: TopTerms,
    #[serde(default, rename = "withField")]
    with_field: Option<DocCount>,
}

pub async fn top_list(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    kind: &str,
    range: &str,
    limit: u64,
    min_level: i64,
) -> HomarrResult<TopList> {
    let field = match kind {
        "rules" => "rule.id",
        "agents" => "agent.name",
        "sourceIps" => "data.srcip",
        "mitreTactics" => "rule.mitre.tactic",
        "mitreTechniques" => "rule.mitre.technique",
        other => {
            return Err(HomarrError::BadRequest(format!(
                "Unknown Wazuh top list '{other}'"
            )))
        }
    };
    range_ms(range)?;
    let mut sub = serde_json::Map::new();
    sub.insert(
        "maxLevel".into(),
        json!({ "max": { "field": "rule.level" } }),
    );
    sub.insert(
        "agents".into(),
        json!({ "cardinality": { "field": "agent.id" } }),
    );
    match kind {
        "rules" => {
            sub.insert(
                "label".into(),
                json!({ "terms": { "field": "rule.description", "size": 1 } }),
            );
        }
        "agents" => {
            sub.insert(
                "detail".into(),
                json!({ "terms": { "field": "agent.id", "size": 1 } }),
            );
        }
        "mitreTechniques" => {
            sub.insert(
                "detail".into(),
                json!({ "terms": { "field": "rule.mitre.id", "size": 6 } }),
            );
        }
        _ => {}
    }
    let mut filter = vec![json!({ "range": { "timestamp": { "gte": format!("now-{range}") } } })];
    if min_level > 0 {
        filter.push(json!({ "range": { "rule.level": { "gte": min_level } } }));
    }
    let response = indexer_search::<Value, TopAggs>(
        rt,
        ctx,
        "wazuh-alerts-*",
        json!({
            "size": 0,
            "track_total_hits": true,
            "query": { "bool": { "filter": filter } },
            "aggs": {
                "top": { "terms": { "field": field, "size": limit }, "aggs": Value::Object(sub) },
                "withField": { "filter": { "exists": { "field": field } } }
            }
        }),
    )
    .await?;
    let hits_total = total_hits(&response.hits.total);
    Ok(map_top_list(kind, range, response.aggregations, hits_total))
}

fn map_top_list(
    kind: &str,
    range: &str,
    aggregations: Option<TopAggs>,
    hits_total: u64,
) -> TopList {
    let (top, with_field) = match aggregations {
        Some(aggs) => (aggs.top, aggs.with_field),
        None => (TopTerms::default(), None),
    };
    let entries = top
        .buckets
        .into_iter()
        .map(|bucket| {
            let mut label = bucket.key.clone();
            let mut detail = None;
            match kind {
                "rules" => {
                    if let Some(first) = bucket.label.as_ref().and_then(|l| l.buckets.first()) {
                        label = first.key.clone();
                    }
                    detail = Some(bucket.key.clone());
                }
                "agents" => {
                    detail = bucket
                        .detail
                        .as_ref()
                        .and_then(|d| d.buckets.first())
                        .map(|b| b.key.clone())
                }
                "mitreTechniques" => {
                    // Technique names and ids are parallel arrays; the id that always co-occurs with the name
                    // belongs to it.
                    detail = bucket
                        .detail
                        .as_ref()
                        .and_then(|d| {
                            d.buckets
                                .iter()
                                .find(|candidate| candidate.doc_count == bucket.doc_count)
                        })
                        .map(|b| b.key.clone());
                }
                _ => {}
            }
            TopListEntry {
                key: bucket.key,
                label,
                detail,
                count: bucket.doc_count,
                max_level: bucket.max_level.value,
                agents: if kind == "agents" {
                    None
                } else {
                    Some(bucket.agents.value.unwrap_or(0.0) as u64)
                },
            }
        })
        .collect();
    TopList {
        kind: kind.to_string(),
        range: range.to_string(),
        total: with_field.map_or(hits_total, |with_field| with_field.doc_count),
        others: top.sum_other_doc_count,
        entries,
    }
}

// ---- security summary --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PeriodBucket {
    #[serde(deserialize_with = "de_key")]
    key: String,
    #[serde(default)]
    doc_count: u64,
    #[serde(default)]
    severity: Buckets,
    #[serde(default, rename = "maxLevel")]
    max_level: Metric,
}

#[derive(Debug, Deserialize, Default)]
struct PeriodBuckets {
    #[serde(default)]
    buckets: Vec<PeriodBucket>,
}

#[derive(Debug, Deserialize)]
struct PeriodsAggs {
    #[serde(default)]
    periods: PeriodBuckets,
}

struct AlertPeriods {
    current: AlertSeverityCounts,
    previous: AlertSeverityCounts,
    max_level: Option<f64>,
}

async fn alert_periods(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    range: &str,
) -> WResult<AlertPeriods> {
    let duration = time_range_ms(range).unwrap_or(0);
    let now = now_ms();
    let response = indexer_search::<Value, PeriodsAggs>(
        rt,
        ctx,
        "wazuh-alerts-*",
        json!({
            "size": 0,
            "query": { "range": { "timestamp": { "gte": now - 2 * duration, "lt": now, "format": "epoch_millis" } } },
            "aggs": { "periods": {
                "date_range": {
                    "field": "timestamp",
                    "format": "epoch_millis",
                    "keyed": false,
                    "ranges": [
                        { "key": "previous", "from": now - 2 * duration, "to": now - duration },
                        { "key": "current", "from": now - duration, "to": now },
                    ],
                },
                "aggs": { "severity": severity_range_aggregation(), "maxLevel": { "max": { "field": "rule.level" } } }
            } }
        }),
    )
    .await?;
    let buckets = response
        .aggregations
        .map(|aggs| aggs.periods.buckets)
        .unwrap_or_default();
    let find = |key: &str| buckets.iter().find(|bucket| bucket.key == key);
    let counts = |bucket: Option<&PeriodBucket>| {
        to_severity_counts(
            bucket.map(|b| b.severity.buckets.as_slice()),
            bucket.map_or(0, |b| b.doc_count),
        )
    };
    Ok(AlertPeriods {
        current: counts(find("current")),
        previous: counts(find("previous")),
        max_level: find("current").and_then(|bucket| bucket.max_level.value),
    })
}

#[derive(Debug, Deserialize)]
struct ClusterHealth {
    #[serde(default)]
    status: Option<String>,
}

pub async fn security_summary(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    range: &str,
) -> HomarrResult<SecuritySummary> {
    range_ms(range)?;
    let Endpoints { api, indexer } = endpoints(ctx);

    let periods_future = async {
        match &indexer {
            Some(_) => alert_periods(rt, ctx, range).await.map(Some),
            None => Ok(None),
        }
    };
    let manager_future = async {
        match &api {
            Some(_) => {
                match api_get::<ApiItems<ManagerInfo>>(rt, ctx, "/manager/info", &[]).await {
                    Ok(data) => data
                        .affected_items
                        .into_iter()
                        .next()
                        .and_then(|info| info.version),
                    Err(error) => {
                        tracing::warn!(integration_id = %ctx.id, reason = error.reason, "Failed to read Wazuh manager info");
                        None
                    }
                }
            }
            None => None,
        }
    };
    let health_future = async {
        match &indexer {
            // Restricted indexer users may not read cluster health; that must not break the widget.
            Some(_) => indexer_get::<ClusterHealth>(rt, ctx, "/_cluster/health")
                .await
                .ok()
                .and_then(|health| health.status),
            None => None,
        }
    };
    let (periods, manager_version, indexer_health) =
        tokio::join!(periods_future, manager_future, health_future);
    let periods = periods?;

    if indexer.is_none() && api.is_none() {
        return Err(WazuhError::new(None, "unauthorized").into());
    }
    if indexer.is_none() && manager_version.is_none() {
        return Err(WazuhError::new(Some("indexer"), "indexerRequired").into());
    }
    Ok(SecuritySummary {
        range: range.to_string(),
        alerts: periods.as_ref().map(|p| p.current),
        previous_alerts: periods.as_ref().map(|p| p.previous),
        max_level: periods.and_then(|p| p.max_level),
        manager_version,
        indexer_health,
    })
}

// ---- vulnerabilities ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CveBucket {
    #[serde(deserialize_with = "de_key")]
    key: String,
    #[serde(default, rename = "maxScore")]
    max_score: Metric,
    #[serde(default)]
    agents: Metric,
    #[serde(default)]
    severity: Buckets,
    #[serde(default)]
    packages: Buckets,
    #[serde(default)]
    reference: Buckets,
}

#[derive(Debug, Deserialize, Default)]
struct CveTop {
    #[serde(default)]
    buckets: Vec<CveBucket>,
}

#[derive(Debug, Deserialize, Default)]
struct CveFilter {
    #[serde(default)]
    top: CveTop,
}

#[derive(Debug, Deserialize)]
struct VulnAgentBucket {
    #[serde(deserialize_with = "de_key")]
    key: String,
    #[serde(default)]
    doc_count: u64,
    #[serde(default)]
    critical: DocCount,
    #[serde(default)]
    high: DocCount,
    #[serde(default)]
    name: Buckets,
}

#[derive(Debug, Deserialize, Default)]
struct VulnAgents {
    #[serde(default)]
    buckets: Vec<VulnAgentBucket>,
}

#[derive(Debug, Deserialize)]
struct VulnAggs {
    #[serde(default)]
    severity: Buckets,
    #[serde(default)]
    agents: Metric,
    #[serde(default)]
    cves: CveFilter,
    #[serde(default, rename = "topAgents")]
    top_agents: VulnAgents,
}

pub async fn vulnerabilities(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    cve_limit: u64,
    agent_limit: u64,
    cve_order: &str,
    min_severity: &str,
) -> HomarrResult<VulnerabilityOverview> {
    let severities: Option<&[&str]> = match min_severity {
        "all" => None,
        "medium" => Some(&["Medium", "High", "Critical"]),
        "high" => Some(&["High", "Critical"]),
        "critical" => Some(&["Critical"]),
        other => {
            return Err(HomarrError::BadRequest(format!(
                "Unknown minimum severity '{other}'"
            )))
        }
    };
    let order = if cve_order == "score" {
        json!([{ "maxScore": "desc" }, { "agents": "desc" }])
    } else {
        json!([{ "agents": "desc" }, { "maxScore": "desc" }])
    };
    let cve_filter = severities.map_or(
        json!({ "match_all": {} }),
        |severities| json!({ "terms": { "vulnerability.severity": severities } }),
    );
    let response = indexer_search::<Value, VulnAggs>(
        rt,
        ctx,
        "wazuh-states-vulnerabilities-*",
        json!({
            "size": 0,
            "track_total_hits": true,
            "aggs": {
                "severity": { "terms": { "field": "vulnerability.severity", "size": 10, "missing": "-" } },
                "agents": { "cardinality": { "field": "agent.id" } },
                "cves": {
                    "filter": cve_filter,
                    "aggs": { "top": {
                        "terms": { "field": "vulnerability.id", "size": cve_limit, "order": order },
                        "aggs": {
                            "maxScore": { "max": { "field": "vulnerability.score.base" } },
                            "agents": { "cardinality": { "field": "agent.id" } },
                            "severity": { "terms": { "field": "vulnerability.severity", "size": 1 } },
                            "packages": { "terms": { "field": "package.name", "size": 3 } },
                            "reference": { "terms": { "field": "vulnerability.scanner.reference", "size": 1 } }
                        }
                    } }
                },
                "topAgents": {
                    "terms": {
                        "field": "agent.id",
                        "size": agent_limit,
                        "order": [{ "critical": "desc" }, { "high": "desc" }, { "_count": "desc" }]
                    },
                    "aggs": {
                        "critical": { "filter": { "term": { "vulnerability.severity": "Critical" } } },
                        "high": { "filter": { "term": { "vulnerability.severity": "High" } } },
                        "name": { "terms": { "field": "agent.name", "size": 1 } }
                    }
                }
            }
        }),
    )
    .await?;
    let total = total_hits(&response.hits.total);
    Ok(map_vulnerabilities(response.aggregations, total))
}

fn map_vulnerabilities(aggregations: Option<VulnAggs>, total: u64) -> VulnerabilityOverview {
    let aggs = aggregations.unwrap_or(VulnAggs {
        severity: Buckets::default(),
        agents: Metric::default(),
        cves: CveFilter::default(),
        top_agents: VulnAgents::default(),
    });
    let mut counts = VulnerabilityCounts::default();
    for bucket in &aggs.severity.buckets {
        match bucket.key.to_lowercase().as_str() {
            "critical" => counts.critical += bucket.doc_count,
            "high" => counts.high += bucket.doc_count,
            "medium" => counts.medium += bucket.doc_count,
            "low" => counts.low += bucket.doc_count,
            _ => counts.unscored += bucket.doc_count,
        }
    }
    VulnerabilityOverview {
        counts,
        total,
        affected_agents: aggs.agents.value.unwrap_or(0.0) as u64,
        top_cves: aggs
            .cves
            .top
            .buckets
            .into_iter()
            .map(|bucket| Cve {
                id: bucket.key,
                severity: bucket
                    .severity
                    .buckets
                    .first()
                    .map_or_else(|| "-".to_string(), |b| b.key.clone()),
                score: bucket.max_score.value,
                agents: bucket.agents.value.unwrap_or(0.0) as u64,
                packages: bucket
                    .packages
                    .buckets
                    .into_iter()
                    .map(|package| package.key)
                    .collect(),
                reference: bucket
                    .reference
                    .buckets
                    .into_iter()
                    .next()
                    .map(|reference| reference.key),
            })
            .collect(),
        top_agents: aggs
            .top_agents
            .buckets
            .into_iter()
            .map(|bucket| VulnerableAgent {
                name: bucket
                    .name
                    .buckets
                    .first()
                    .map_or_else(|| bucket.key.clone(), |b| b.key.clone()),
                id: bucket.key,
                total: bucket.doc_count,
                critical: bucket.critical.doc_count,
                high: bucket.high.doc_count,
            })
            .collect(),
    }
}

// ---- file integrity monitoring -----------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FimAggs {
    #[serde(default)]
    events: Buckets,
}

pub async fn fim_events(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    range: &str,
    event: &str,
    limit: u64,
    include_registry: bool,
) -> HomarrResult<FimOverview> {
    range_ms(range)?;
    let mut body = json!({
        "size": limit,
        "sort": [{ "timestamp": { "order": "desc" } }],
        "_source": [
            "id", "timestamp", "rule.level", "rule.description", "rule.groups", "agent.id", "agent.name",
            "syscheck.path", "syscheck.event", "syscheck.value_name", "syscheck.uname_after",
            "syscheck.audit.user.name"
        ],
        "query": { "bool": {
            "filter": [
                { "term": { "rule.groups": "syscheck" } },
                { "range": { "timestamp": { "gte": format!("now-{range}") } } }
            ],
            "must_not": if include_registry { json!([]) } else { json!([{ "term": { "rule.groups": "syscheck_registry" } }]) }
        } },
        "aggs": { "events": { "terms": { "field": "syscheck.event", "size": 5 } } }
    });
    // The event filter only narrows the list; the counts in the header always show all three events.
    if event != "all" {
        body["post_filter"] = json!({ "term": { "syscheck.event": event } });
    }
    let response = indexer_search::<AlertSource, FimAggs>(rt, ctx, "wazuh-alerts-*", body).await?;
    Ok(map_fim(range, response))
}

fn map_fim(range: &str, response: IndexerSearchResponse<AlertSource, FimAggs>) -> FimOverview {
    let mut counts = BTreeMap::from([
        ("added".to_string(), 0),
        ("modified".to_string(), 0),
        ("deleted".to_string(), 0),
    ]);
    for bucket in response
        .aggregations
        .map(|aggs| aggs.events.buckets)
        .unwrap_or_default()
    {
        if let Some(count) = counts.get_mut(&bucket.key) {
            *count = bucket.doc_count;
        }
    }
    let entries = response
        .hits
        .hits
        .into_iter()
        .map(|hit| {
            let source = hit.source;
            let syscheck = source.syscheck.unwrap_or_default();
            let rule = source.rule.unwrap_or_default();
            let agent = source.agent.unwrap_or_default();
            let event = match syscheck.event.as_deref() {
                Some(event @ ("added" | "modified" | "deleted")) => event.to_string(),
                _ => "unknown".to_string(),
            };
            FimEntry {
                id: hit.id,
                alert_id: source.id,
                timestamp: source
                    .timestamp
                    .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into()),
                event,
                path: syscheck.path.unwrap_or_default(),
                value_name: syscheck.value_name,
                is_registry: rule
                    .groups
                    .map(StringOrList::into_vec)
                    .is_some_and(|groups| groups.iter().any(|group| group == "syscheck_registry")),
                agent_id: agent.id,
                agent_name: agent.name,
                level: rule.level.unwrap_or(0),
                description: rule.description.unwrap_or_default(),
                user: syscheck.uname_after.or_else(|| {
                    syscheck
                        .audit
                        .and_then(|audit| audit.user)
                        .and_then(|user| user.name)
                }),
            }
        })
        .collect();
    FimOverview {
        range: range.to_string(),
        counts,
        entries,
    }
}

// ---- authentication failures -------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AuthAgentBucket {
    #[serde(deserialize_with = "de_key")]
    key: String,
    #[serde(default)]
    doc_count: u64,
    #[serde(default, rename = "bruteForce")]
    brute_force: DocCount,
    #[serde(default)]
    id: Buckets,
    #[serde(default)]
    series: NumBuckets,
    #[serde(default, rename = "sourceIps")]
    source_ips: Buckets,
    #[serde(default, rename = "lastSeen")]
    last_seen: Metric,
}

#[derive(Debug, Deserialize, Default)]
struct AuthAgents {
    #[serde(default)]
    buckets: Vec<AuthAgentBucket>,
}

#[derive(Debug, Deserialize)]
struct AuthAggs {
    #[serde(default, rename = "bruteForce")]
    brute_force: DocCount,
    #[serde(default, rename = "sourceIps")]
    source_ips: Metric,
    #[serde(default)]
    series: NumBuckets,
    #[serde(default)]
    agents: AuthAgents,
}

pub async fn auth_failures(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    hours: u64,
    limit: u64,
) -> HomarrResult<AuthFailures> {
    let hours = hours.clamp(1, 24 * 365);
    let limit = limit.clamp(1, 500);
    let hours_i = hours as i64;
    let bucket_hours = ((hours_i + 23) / 24).max(1);
    let bucket_ms = bucket_hours * 60 * 60 * 1000;
    let end = (now_ms() + bucket_ms - 1).div_euclid(bucket_ms) * bucket_ms;
    let start = end - ((hours_i + bucket_hours - 1) / bucket_hours) * bucket_ms;
    let histogram = json!({
        "date_histogram": {
            "field": "timestamp",
            "fixed_interval": format!("{bucket_hours}h"),
            "min_doc_count": 0,
            "extended_bounds": { "min": start, "max": end - bucket_ms },
        }
    });
    let response = indexer_search::<Value, AuthAggs>(
        rt,
        ctx,
        "wazuh-alerts-*",
        json!({
            "size": 0,
            "track_total_hits": true,
            "query": { "bool": { "filter": [
                { "range": { "timestamp": { "gte": start, "lt": end, "format": "epoch_millis" } } },
                queries::auth_failure_query()
            ] } },
            "aggs": {
                "bruteForce": { "filter": queries::brute_force_query() },
                "sourceIps": { "cardinality": { "field": "data.srcip" } },
                "series": histogram.clone(),
                "agents": {
                    "terms": { "field": "agent.name", "size": limit },
                    "aggs": {
                        "bruteForce": { "filter": queries::brute_force_query() },
                        "id": { "terms": { "field": "agent.id", "size": 1 } },
                        "series": histogram,
                        "sourceIps": { "terms": { "field": "data.srcip", "size": 3 } },
                        "lastSeen": { "max": { "field": "timestamp" } }
                    }
                }
            }
        }),
    )
    .await?;
    let total = total_hits(&response.hits.total);
    Ok(map_auth_failures(hours, total, response.aggregations))
}

fn map_auth_failures(hours: u64, total: u64, aggregations: Option<AuthAggs>) -> AuthFailures {
    let aggs = aggregations.unwrap_or(AuthAggs {
        brute_force: DocCount::default(),
        source_ips: Metric::default(),
        series: NumBuckets::default(),
        agents: AuthAgents::default(),
    });
    let brute_force = aggs.brute_force.doc_count;
    AuthFailures {
        hours,
        totals: AuthFailureTotals {
            total,
            brute_force,
            failed: total.saturating_sub(brute_force),
            source_ips: aggs.source_ips.value.unwrap_or(0.0) as u64,
        },
        series: aggs
            .series
            .buckets
            .iter()
            .map(|bucket| bucket.doc_count)
            .collect(),
        agents: aggs
            .agents
            .buckets
            .into_iter()
            .map(|bucket| AuthFailureAgent {
                agent_id: bucket.id.buckets.first().map(|b| b.key.clone()),
                total: bucket.doc_count,
                brute_force: bucket.brute_force.doc_count,
                failed: bucket
                    .doc_count
                    .saturating_sub(bucket.brute_force.doc_count),
                series: bucket
                    .series
                    .buckets
                    .iter()
                    .map(|point| point.doc_count)
                    .collect(),
                top_source_ips: bucket
                    .source_ips
                    .buckets
                    .into_iter()
                    .map(|ip| SourceIpCount {
                        ip: ip.key,
                        count: ip.doc_count,
                    })
                    .collect(),
                last_seen: bucket.last_seen.value.map(|value| iso(value as i64)),
                agent_name: bucket.key,
            })
            .collect(),
    }
}

// ---- generic data operations -------------------------------------------------------------------

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    let range = input_str(input, "range").unwrap_or("24h");
    let min_level = input.get("minLevel").and_then(Value::as_i64).unwrap_or(0);
    let limit = input_u64(input, "limit").map(|value| value.clamp(1, 500));
    let hours = input_u64(input, "hours").unwrap_or(24).clamp(1, 24 * 365);
    match operation {
        "getAgentsOverview" => to_json(&agents_overview(rt, ctx).await?),
        "getRecentAlerts" => {
            to_json(&recent_alerts(rt, ctx, min_level, limit.unwrap_or(10), hours).await?)
        }
        "getAlertTimeline" => to_json(
            &alert_timeline(
                rt,
                ctx,
                range,
                input_str(input, "interval").unwrap_or("auto"),
                min_level,
            )
            .await?,
        ),
        "getTopList" => to_json(
            &top_list(
                rt,
                ctx,
                input_str(input, "kind").unwrap_or("rules"),
                range,
                limit.unwrap_or(5),
                min_level,
            )
            .await?,
        ),
        "getSecuritySummary" => to_json(&security_summary(rt, ctx, range).await?),
        "getVulnerabilities" => to_json(
            &vulnerabilities(
                rt,
                ctx,
                input_u64(input, "cveLimit").unwrap_or(5).clamp(1, 500),
                input_u64(input, "agentLimit").unwrap_or(5).clamp(1, 500),
                input_str(input, "cveOrder").unwrap_or("score"),
                input_str(input, "minSeverity").unwrap_or("all"),
            )
            .await?,
        ),
        "getFimEvents" => to_json(
            &fim_events(
                rt,
                ctx,
                range,
                input_str(input, "event").unwrap_or("all"),
                limit.unwrap_or(10),
                input
                    .get("includeRegistry")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            )
            .await?,
        ),
        "getAuthFailures" => to_json(&auth_failures(rt, ctx, hours, limit.unwrap_or(5)).await?),
        "getDashboardUrl" => to_json(&dashboard_url(ctx)),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx(url: &str, external: Option<&str>, secrets: &[(&str, &str)]) -> IntegrationContext {
        IntegrationContext {
            id: "w".into(),
            name: "Wazuh".into(),
            url: url.into(),
            external_url: external.map(str::to_string),
            kind: IntegrationKind::Wazuh,
            secrets: secrets
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn endpoints_follow_credential_alternatives() {
        let api_only = endpoints(&ctx(
            "https://wazuh:55000/",
            None,
            &[("username", "u"), ("password", "p")],
        ));
        assert_eq!(api_only.api.unwrap().url, "https://wazuh:55000");
        assert!(api_only.indexer.is_none());

        let both = endpoints(&ctx(
            "https://wazuh:55000",
            None,
            &[
                ("username", "u"),
                ("password", "p"),
                ("wazuhIndexerUrl", "https://idx:9200/"),
                ("wazuhIndexerUsername", "iu"),
                ("wazuhIndexerPassword", "ip"),
            ],
        ));
        assert_eq!(both.indexer.unwrap().url, "https://idx:9200");

        // Indexer credentials without an indexer URL use the integration URL, but only without API credentials.
        let indexer_only = endpoints(&ctx(
            "https://idx:9200",
            None,
            &[
                ("wazuhIndexerUsername", "iu"),
                ("wazuhIndexerPassword", "ip"),
            ],
        ));
        assert!(indexer_only.api.is_none());
        assert_eq!(indexer_only.indexer.unwrap().url, "https://idx:9200");
        let api_with_partial = endpoints(&ctx(
            "https://wazuh:55000",
            None,
            &[
                ("username", "u"),
                ("password", "p"),
                ("wazuhIndexerUsername", "iu"),
                ("wazuhIndexerPassword", "ip"),
            ],
        ));
        assert!(api_with_partial.indexer.is_none());
    }

    #[test]
    fn dashboard_url_requires_a_different_external_url() {
        assert_eq!(
            dashboard_url(&ctx(
                "https://idx:9200",
                Some("https://wazuh.example.com/"),
                &[]
            ))
            .as_deref(),
            Some("https://wazuh.example.com")
        );
        assert_eq!(
            dashboard_url(&ctx("https://idx:9200", Some("https://idx:9200/"), &[])),
            None
        );
        assert_eq!(dashboard_url(&ctx("https://idx:9200", None, &[])), None);
    }

    #[test]
    fn indexer_overview_keeps_newest_document_per_agent() {
        let hits: Vec<IndexerHit<MonitoringSource>> = serde_json::from_value(json!([
            {"_id": "1", "_source": {"id": "000", "name": "manager", "status": "active"}},
            {"_id": "2", "_source": {"id": "001", "name": "b-host", "status": "active", "version": "Wazuh v4.9.0", "os": {"platform": "Ubuntu"}, "group": "default"}},
            {"_id": "3", "_source": {"id": "001", "name": "old", "status": "disconnected"}},
            {"_id": "4", "_source": {"id": "002", "name": "A-host", "status": "never_connected"}},
            {"_id": "5", "_source": {"name": "no-id"}}
        ]))
        .unwrap();
        let overview = build_indexer_overview(hits, 1_700_000_000_000);
        assert_eq!(overview.counts.total, 2);
        assert_eq!(
            (overview.counts.active, overview.counts.never_connected),
            (1, 1)
        );
        assert_eq!(overview.agents[0].name, "A-host");
        assert_eq!(overview.agents[1].os_platform.as_deref(), Some("ubuntu"));
        assert_eq!(overview.agents[1].version.as_deref(), Some("v4.9.0"));
        assert_eq!(overview.agents[1].groups, vec!["default"]);
        assert_eq!(overview.as_of, "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn top_list_maps_rule_and_technique_details() {
        let aggs: TopAggs = serde_json::from_value(json!({
            "top": {"sum_other_doc_count": 7, "buckets": [
                {"key": "5712", "doc_count": 10, "maxLevel": {"value": 10}, "agents": {"value": 2},
                 "label": {"buckets": [{"key": "sshd brute force", "doc_count": 10}]}}
            ]},
            "withField": {"doc_count": 25}
        }))
        .unwrap();
        let list = map_top_list("rules", "24h", Some(aggs), 99);
        assert_eq!(list.total, 25);
        assert_eq!(list.others, 7);
        assert_eq!(list.entries[0].label, "sshd brute force");
        assert_eq!(list.entries[0].detail.as_deref(), Some("5712"));
        assert_eq!(list.entries[0].agents, Some(2));

        let aggs: TopAggs = serde_json::from_value(json!({
            "top": {"buckets": [
                {"key": "Brute Force", "doc_count": 4, "maxLevel": {"value": null}, "agents": {"value": 1},
                 "detail": {"buckets": [{"key": "T1000", "doc_count": 2}, {"key": "T1110", "doc_count": 4}]}}
            ]}
        }))
        .unwrap();
        let list = map_top_list("mitreTechniques", "24h", Some(aggs), 4);
        assert_eq!(list.total, 4);
        assert_eq!(list.entries[0].detail.as_deref(), Some("T1110"));
    }

    #[test]
    fn vulnerabilities_bucket_severities() {
        let aggs: VulnAggs = serde_json::from_value(json!({
            "severity": {"buckets": [
                {"key": "Critical", "doc_count": 2}, {"key": "high", "doc_count": 3}, {"key": "-", "doc_count": 1}
            ]},
            "agents": {"value": 4},
            "cves": {"top": {"buckets": [
                {"key": "CVE-1", "maxScore": {"value": 9.8}, "agents": {"value": 2},
                 "severity": {"buckets": [{"key": "Critical", "doc_count": 2}]},
                 "packages": {"buckets": [{"key": "openssl", "doc_count": 1}]},
                 "reference": {"buckets": []}}
            ]}},
            "topAgents": {"buckets": [
                {"key": "001", "doc_count": 5, "critical": {"doc_count": 2}, "high": {"doc_count": 3}, "name": {"buckets": []}}
            ]}
        }))
        .unwrap();
        let overview = map_vulnerabilities(Some(aggs), 6);
        assert_eq!(
            (
                overview.counts.critical,
                overview.counts.high,
                overview.counts.unscored
            ),
            (2, 3, 1)
        );
        assert_eq!(overview.top_cves[0].packages, vec!["openssl"]);
        assert_eq!(overview.top_cves[0].reference, None);
        assert_eq!(overview.top_agents[0].name, "001");
        assert_eq!(overview.affected_agents, 4);
    }

    #[test]
    fn auth_failures_split_brute_force_from_failed() {
        let aggs: AuthAggs = serde_json::from_value(json!({
            "bruteForce": {"doc_count": 3},
            "sourceIps": {"value": 2},
            "series": {"buckets": [{"key": 1.0, "doc_count": 4}, {"key": 2.0, "doc_count": 6}]},
            "agents": {"buckets": [{
                "key": "web-01", "doc_count": 10, "bruteForce": {"doc_count": 3},
                "id": {"buckets": [{"key": "003", "doc_count": 10}]},
                "series": {"buckets": [{"key": 1.0, "doc_count": 10}]},
                "sourceIps": {"buckets": [{"key": "203.0.113.7", "doc_count": 8}]},
                "lastSeen": {"value": 1700000000000.0}
            }]}
        }))
        .unwrap();
        let result = map_auth_failures(24, 10, Some(aggs));
        assert_eq!((result.totals.failed, result.totals.brute_force), (7, 3));
        assert_eq!(result.series, vec![4, 6]);
        assert_eq!(result.agents[0].agent_id.as_deref(), Some("003"));
        assert_eq!(
            result.agents[0].last_seen.as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
    }

    #[test]
    fn wazuh_errors_are_user_presentable() {
        let error: HomarrError = WazuhError {
            target: Some("indexer"),
            reason: "status",
            status: Some(500),
            detail: None,
        }
        .into();
        assert!(error.to_string().contains("indexer") && error.to_string().contains("500"));
        let error: HomarrError = WazuhError::new(Some("indexer"), "indexerRequired").into();
        assert!(error.to_string().contains("indexer credentials"));
    }
}
