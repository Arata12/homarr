//! Wazuh indexer query building and mapping helpers (`wazuh-queries.ts`).

use serde_json::{json, Value};

use super::types::{
    Alert, AlertSeverityCounts, AlertSource, Bucket, IndexerHit, Mitre, StringOrList, Total,
};

pub fn time_range_ms(range: &str) -> Option<i64> {
    Some(match range {
        "1h" => 60 * 60 * 1000,
        "24h" => 24 * 60 * 60 * 1000,
        "7d" => 7 * 24 * 60 * 60 * 1000,
        "30d" => 30 * 24 * 60 * 60 * 1000,
        _ => return None,
    })
}

const INTERVALS: [(&str, i64); 9] = [
    ("1m", 60_000),
    ("5m", 5 * 60_000),
    ("15m", 15 * 60_000),
    ("30m", 30 * 60_000),
    ("1h", 3_600_000),
    ("3h", 3 * 3_600_000),
    ("6h", 6 * 3_600_000),
    ("12h", 12 * 3_600_000),
    ("1d", 86_400_000),
];

fn auto_interval(range: &str) -> &'static str {
    match range {
        "1h" => "1m",
        "24h" => "30m",
        "7d" => "3h",
        _ => "12h",
    }
}

/// Upper bound for histogram buckets so a small interval on a long range cannot overload the indexer.
const MAX_TIMELINE_BUCKETS: i64 = 400;

pub fn resolve_timeline_interval_ms(range_ms: i64, range: &str, interval: &str) -> i64 {
    let name = if interval == "auto" {
        auto_interval(range)
    } else {
        interval
    };
    let requested = INTERVALS
        .iter()
        .find(|(key, _)| *key == name)
        .map_or(INTERVALS[4].1, |(_, ms)| *ms);
    let minimum = (range_ms + MAX_TIMELINE_BUCKETS - 1) / MAX_TIMELINE_BUCKETS;
    if requested >= minimum {
        return requested;
    }
    // Pick the smallest predefined interval that keeps the bucket count in bounds.
    INTERVALS
        .iter()
        .map(|(_, ms)| *ms)
        .find(|ms| *ms >= minimum)
        .unwrap_or(86_400_000)
}

/// Rule level buckets used by the Wazuh dashboard.
pub fn severity_range_aggregation() -> Value {
    json!({
        "range": {
            "field": "rule.level",
            "keyed": false,
            "ranges": [
                { "key": "low", "to": 7 },
                { "key": "medium", "from": 7, "to": 12 },
                { "key": "high", "from": 12, "to": 15 },
                { "key": "critical", "from": 15 }
            ]
        }
    })
}

pub fn to_severity_counts(buckets: Option<&[Bucket]>, total: u64) -> AlertSeverityCounts {
    let count = |key: &str| {
        buckets
            .and_then(|buckets| buckets.iter().find(|bucket| bucket.key == key))
            .map_or(0, |bucket| bucket.doc_count)
    };
    AlertSeverityCounts {
        total,
        low: count("low"),
        medium: count("medium"),
        high: count("high"),
        critical: count("critical"),
    }
}

pub fn total_hits(total: &Total) -> u64 {
    total.get()
}

/// Auth failure groups. Brute force = Wazuh correlation rules (authentication_failures) or MITRE T1110.
pub fn auth_failure_query() -> Value {
    json!({
        "bool": {
            "should": [
                { "terms": { "rule.groups": ["authentication_failed", "authentication_failures", "invalid_login"] } },
                { "term": { "rule.mitre.id": "T1110" } }
            ],
            "minimum_should_match": 1
        }
    })
}

pub fn brute_force_query() -> Value {
    json!({
        "bool": {
            "should": [
                { "term": { "rule.groups": "authentication_failures" } },
                { "term": { "rule.mitre.id": "T1110" } }
            ],
            "minimum_should_match": 1
        }
    })
}

pub const ALERT_SOURCE_FIELDS: [&str; 12] = [
    "id",
    "timestamp",
    "rule.id",
    "rule.level",
    "rule.description",
    "rule.groups",
    "rule.mitre.id",
    "rule.mitre.tactic",
    "rule.mitre.technique",
    "agent.id",
    "agent.name",
    "data.srcip",
];

fn as_vec(value: Option<StringOrList>) -> Vec<String> {
    value.map(StringOrList::into_vec).unwrap_or_default()
}

pub fn map_alert_hit(hit: IndexerHit<AlertSource>) -> Alert {
    let source = hit.source;
    let rule = source.rule.unwrap_or_default();
    let mitre = rule.mitre.unwrap_or_default();
    let agent = source.agent.unwrap_or_default();
    Alert {
        id: hit.id,
        alert_id: source.id,
        timestamp: source
            .timestamp
            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string()),
        level: rule.level.unwrap_or(0),
        rule_id: rule.id,
        description: rule.description.unwrap_or_default(),
        agent_id: agent.id,
        agent_name: agent.name,
        source_ip: source.data.and_then(|data| data.srcip),
        groups: as_vec(rule.groups),
        mitre: Mitre {
            ids: as_vec(mitre.id),
            tactics: as_vec(mitre.tactic),
            techniques: as_vec(mitre.technique),
        },
    }
}

pub fn normalize_agent_status(status: Option<&str>) -> &'static str {
    match status {
        Some("active") => "active",
        Some("disconnected") => "disconnected",
        Some("never_connected") => "never_connected",
        Some("pending") => "pending",
        _ => "unknown",
    }
}

pub fn normalize_version(version: Option<&str>) -> Option<String> {
    version.map(|version| {
        let trimmed = version.trim_start();
        match trimmed.get(..5) {
            Some(prefix)
                if prefix.eq_ignore_ascii_case("wazuh")
                    && trimmed[5..].starts_with(char::is_whitespace) =>
            {
                trimmed[5..].trim_start().to_string()
            }
            _ => version.to_string(),
        }
    })
}

/// Plain text body of a Wazuh alert in the notifications widget.
pub fn format_alert_notification_body(alert: &Alert) -> String {
    let mut first = format!("Level {}", alert.level);
    if let Some(rule_id) = alert.rule_id.as_deref().filter(|id| !id.is_empty()) {
        first.push_str(&format!(" · Rule {rule_id}"));
    }
    let mut lines = vec![first];
    let agent = alert
        .agent_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .or(alert.agent_id.as_deref().filter(|id| !id.is_empty()));
    if let Some(agent) = agent {
        let mut line = format!("Agent: {agent}");
        if let (Some(_), Some(id)) = (
            alert.agent_name.as_deref().filter(|n| !n.is_empty()),
            alert.agent_id.as_deref().filter(|i| !i.is_empty()),
        ) {
            line.push_str(&format!(" ({id})"));
        }
        lines.push(line);
    }
    if let Some(ip) = alert.source_ip.as_deref().filter(|ip| !ip.is_empty()) {
        lines.push(format!("Source IP: {ip}"));
    }
    if !alert.mitre.ids.is_empty() {
        lines.push(format!("MITRE ATT&CK: {}", alert.mitre.ids.join(", ")));
    }
    lines.join("\n")
}

/// `encodeURIComponent`.
fn encode_component(value: &str) -> String {
    const UNRESERVED: &[u8] = b"-_.!~*'()";
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn kuery(query: &str) -> String {
    encode_component(&format!(
        "(filters:!(),query:(language:kuery,query:'{}'))",
        query.replace('!', "!!").replace('\'', "!'")
    ))
}

/// Link to an alert in the Wazuh dashboard (4.9+ routes): the events view filtered by the alert id within five
/// minutes of the alert.
pub fn create_alert_dashboard_url(dashboard_url: &str, alert: &Alert) -> String {
    let time = chrono::DateTime::parse_from_rfc3339(&alert.timestamp)
        .map(|time| time.timestamp_millis())
        .unwrap_or(0);
    let window = 5 * 60 * 1000;
    let iso = |millis: i64| {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    let mut url = format!(
        "{}/app/threat-hunting#/overview/?tab=general&tabView=events",
        dashboard_url.trim_end_matches('/')
    );
    if let Some(agent_id) = alert.agent_id.as_deref().filter(|id| !id.is_empty()) {
        url.push_str(&format!("&agentId={}", encode_component(agent_id)));
    }
    if let Some(alert_id) = alert.alert_id.as_deref().filter(|id| !id.is_empty()) {
        url.push_str(&format!("&_q={}", kuery(&format!("id:\"{alert_id}\""))));
    }
    url.push_str(&format!(
        "&_g={}",
        encode_component(&format!(
            "(time:(from:'{}',to:'{}'))",
            iso(time - window),
            iso(time + window)
        ))
    ));
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert() -> Alert {
        map_alert_hit(
            serde_json::from_value(json!({
                "_id": "doc-1",
                "_source": {
                    "id": "1790466949.5285465",
                    "timestamp": "2026-10-06T10:00:00.000Z",
                    "rule": {"id": "5712", "level": 12, "description": "sshd: brute force trying to get access to the system.", "mitre": {"id": ["T1110"]}},
                    "agent": {"id": "003", "name": "web-01"},
                    "data": {"srcip": "203.0.113.7"}
                }
            }))
            .unwrap(),
        )
    }

    #[test]
    fn alert_notification_body_lists_context() {
        assert_eq!(
            format_alert_notification_body(&alert()),
            "Level 12 · Rule 5712\nAgent: web-01 (003)\nSource IP: 203.0.113.7\nMITRE ATT&CK: T1110"
        );
    }

    #[test]
    fn minimal_alert_has_plain_body_and_defaults() {
        let minimal = map_alert_hit(
            serde_json::from_value(json!({"_id": "doc-2", "_source": {"timestamp": "2026-10-06T10:00:00.000Z", "rule": {"level": 13}}})).unwrap(),
        );
        assert_eq!(format_alert_notification_body(&minimal), "Level 13");
        assert_eq!(minimal.description, "");
        assert!(minimal.groups.is_empty());
    }

    #[test]
    fn dashboard_url_links_alert_window() {
        let url = create_alert_dashboard_url("https://wazuh.example.com/", &alert());
        assert!(url.starts_with(
            "https://wazuh.example.com/app/threat-hunting#/overview/?tab=general&tabView=events"
        ));
        assert!(url.contains("agentId=003"));
        assert!(url.contains("_q=(filters%3A!()%2Cquery%3A(language%3Akuery%2Cquery%3A'id%3A%22"));
        assert!(url.contains("2026-10-06T09%3A55%3A00.000Z"));
    }

    #[test]
    fn timeline_interval_is_bounded() {
        let day = time_range_ms("24h").unwrap();
        assert_eq!(
            resolve_timeline_interval_ms(day, "24h", "auto"),
            30 * 60_000
        );
        // 1m over 30d would be 43200 buckets; the smallest interval keeping <= 400 buckets is 3h.
        let month = time_range_ms("30d").unwrap();
        assert_eq!(
            resolve_timeline_interval_ms(month, "30d", "1m"),
            3 * 3_600_000
        );
        assert_eq!(resolve_timeline_interval_ms(month, "30d", "1d"), 86_400_000);
    }

    #[test]
    fn severity_counts_default_missing_buckets_to_zero() {
        let buckets = vec![Bucket {
            key: "high".into(),
            doc_count: 4,
        }];
        let counts = to_severity_counts(Some(&buckets), 9);
        assert_eq!((counts.total, counts.high, counts.low), (9, 4, 0));
        assert_eq!(to_severity_counts(None, 0), AlertSeverityCounts::default());
    }

    #[test]
    fn normalizes_versions_and_statuses() {
        assert_eq!(
            normalize_version(Some("Wazuh v4.9.0")).as_deref(),
            Some("v4.9.0")
        );
        assert_eq!(normalize_version(Some("4.9.0")).as_deref(), Some("4.9.0"));
        assert_eq!(normalize_agent_status(Some("active")), "active");
        assert_eq!(normalize_agent_status(Some("weird")), "unknown");
        assert_eq!(super::super::types::severity_for_level(15), "critical");
        assert_eq!(super::super::types::severity_for_level(7), "medium");
    }
}
