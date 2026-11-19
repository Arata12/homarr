//! Wazuh public data types (`wazuh-types.ts`) plus the raw API/indexer response shapes that are read.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

pub const TIME_RANGES: [&str; 4] = ["1h", "24h", "7d", "30d"];
pub const TIMELINE_INTERVALS: [&str; 10] = [
    "auto", "1m", "5m", "15m", "30m", "1h", "3h", "6h", "12h", "1d",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DataSource {
    Api,
    Indexer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct AlertSeverityCounts {
    pub total: u64,
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCounts {
    pub total: u64,
    pub active: u64,
    pub disconnected: u64,
    pub never_connected: u64,
    pub pending: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// `active`, `disconnected`, `never_connected`, `pending` or `unknown`.
    pub status: String,
    pub ip: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub os_platform: Option<String>,
    pub version: Option<String>,
    pub last_keep_alive: Option<String>,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsOverview {
    pub source: DataSource,
    pub as_of: String,
    pub counts: AgentCounts,
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Mitre {
    pub ids: Vec<String>,
    pub tactics: Vec<String>,
    pub techniques: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub alert_id: Option<String>,
    pub timestamp: String,
    pub level: i64,
    pub rule_id: Option<String>,
    pub description: String,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub source_ip: Option<String>,
    pub groups: Vec<String>,
    pub mitre: Mitre,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySummary {
    pub range: String,
    pub alerts: Option<AlertSeverityCounts>,
    pub previous_alerts: Option<AlertSeverityCounts>,
    pub max_level: Option<f64>,
    pub manager_version: Option<String>,
    pub indexer_health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimelinePoint {
    pub timestamp: i64,
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    pub critical: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertTimeline {
    pub range: String,
    pub interval_ms: i64,
    pub totals: AlertSeverityCounts,
    pub points: Vec<TimelinePoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopListEntry {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    pub count: u64,
    pub max_level: Option<f64>,
    pub agents: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopList {
    pub kind: String,
    pub range: String,
    pub total: u64,
    pub others: u64,
    pub entries: Vec<TopListEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cve {
    pub id: String,
    pub severity: String,
    pub score: Option<f64>,
    pub agents: u64,
    pub packages: Vec<String>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VulnerableAgent {
    pub id: String,
    pub name: String,
    pub total: u64,
    pub critical: u64,
    pub high: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VulnerabilityCounts {
    pub critical: u64,
    pub high: u64,
    pub medium: u64,
    pub low: u64,
    pub unscored: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VulnerabilityOverview {
    pub counts: VulnerabilityCounts,
    pub total: u64,
    pub affected_agents: u64,
    pub top_cves: Vec<Cve>,
    pub top_agents: Vec<VulnerableAgent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FimEntry {
    pub id: String,
    pub alert_id: Option<String>,
    pub timestamp: String,
    /// `added`, `modified`, `deleted` or `unknown`.
    pub event: String,
    pub path: String,
    pub value_name: Option<String>,
    pub is_registry: bool,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub level: i64,
    pub description: String,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FimOverview {
    pub range: String,
    pub counts: BTreeMap<String, u64>,
    pub entries: Vec<FimEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceIpCount {
    pub ip: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthFailureAgent {
    pub agent_id: Option<String>,
    pub agent_name: String,
    pub failed: u64,
    pub brute_force: u64,
    pub total: u64,
    pub series: Vec<u64>,
    pub top_source_ips: Vec<SourceIpCount>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthFailureTotals {
    pub failed: u64,
    pub brute_force: u64,
    pub total: u64,
    pub source_ips: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthFailures {
    pub hours: u64,
    pub totals: AuthFailureTotals,
    pub series: Vec<u64>,
    pub agents: Vec<AuthFailureAgent>,
}

/// Levels follow the Wazuh dashboard buckets: low 0-6, medium 7-11, high 12-14, critical 15+.
pub fn severity_for_level(level: i64) -> &'static str {
    match level {
        15.. => "critical",
        12..=14 => "high",
        7..=11 => "medium",
        _ => "low",
    }
}

// ---- raw response shapes ---------------------------------------------------------------------

/// Accepts a string or a number as bucket key and turns it into a string.
pub fn de_key<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "unsupported bucket key {other}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}

#[derive(Debug, Deserialize, Default)]
pub struct ApiAgentsSummaryStatus {
    #[serde(default)]
    pub connection: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
pub struct ApiItems<T> {
    pub affected_items: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Os {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiAgent {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, rename = "lastKeepAlive")]
    pub last_keep_alive: Option<String>,
    #[serde(default)]
    pub group: Option<StringOrList>,
    #[serde(default)]
    pub os: Option<Os>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    One(String),
    Many(Vec<String>),
}

impl StringOrList {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(one) => vec![one],
            Self::Many(many) => many,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ManagerInfo {
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IndexerHit<S> {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_source")]
    pub source: S,
}

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum Total {
    Number(u64),
    Object {
        value: u64,
    },
    #[default]
    Missing,
}

impl Total {
    pub fn get(&self) -> u64 {
        match self {
            Self::Number(value) | Self::Object { value } => *value,
            Self::Missing => 0,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct IndexerHits<S> {
    #[serde(default)]
    pub total: Total,
    #[serde(default = "Vec::new")]
    pub hits: Vec<IndexerHit<S>>,
}

#[derive(Debug, Deserialize)]
pub struct IndexerSearchResponse<S, A> {
    pub hits: IndexerHits<S>,
    pub aggregations: Option<A>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MonitoringSource {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, rename = "lastKeepAlive")]
    pub last_keep_alive: Option<String>,
    #[serde(default)]
    pub group: Option<StringOrList>,
    #[serde(default)]
    pub os: Option<Os>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MitreSource {
    #[serde(default)]
    pub id: Option<StringOrList>,
    #[serde(default)]
    pub tactic: Option<StringOrList>,
    #[serde(default)]
    pub technique: Option<StringOrList>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuleSource {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub groups: Option<StringOrList>,
    #[serde(default)]
    pub mitre: Option<MitreSource>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AgentSource {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DataSourceFields {
    #[serde(default)]
    pub srcip: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuditUser {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Audit {
    #[serde(default)]
    pub user: Option<AuditUser>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SyscheckSource {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub value_name: Option<String>,
    #[serde(default)]
    pub uname_after: Option<String>,
    #[serde(default)]
    pub audit: Option<Audit>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AlertSource {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub rule: Option<RuleSource>,
    #[serde(default)]
    pub agent: Option<AgentSource>,
    #[serde(default)]
    pub data: Option<DataSourceFields>,
    #[serde(default)]
    pub syscheck: Option<SyscheckSource>,
}

// ---- aggregation shapes ----------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Bucket {
    #[serde(deserialize_with = "de_key")]
    pub key: String,
    #[serde(default)]
    pub doc_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct NumBucket {
    pub key: f64,
    #[serde(default)]
    pub doc_count: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct Buckets {
    #[serde(default)]
    pub buckets: Vec<Bucket>,
}

#[derive(Debug, Deserialize, Default)]
pub struct NumBuckets {
    #[serde(default)]
    pub buckets: Vec<NumBucket>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Metric {
    #[serde(default)]
    pub value: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DocCount {
    #[serde(default)]
    pub doc_count: u64,
}
