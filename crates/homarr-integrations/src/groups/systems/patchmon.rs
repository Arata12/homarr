//! PatchMon (`patchmon/patchmon-integration.ts`): host patch statistics.

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, basic_auth};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const STATS_REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchMonOsDistributionEntry {
    pub name: String,
    pub count: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchMonStats {
    pub total_hosts: f64,
    pub hosts_needing_updates: f64,
    pub security_updates: f64,
    pub up_to_date_hosts: f64,
    pub hosts_with_security_updates: f64,
    pub recent_updates_24h: f64,
    pub total_outdated_packages: f64,
    pub total_repos: f64,
    pub last_updated: String,
    pub os_distribution: Vec<PatchMonOsDistributionEntry>,
}

fn stats_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<reqwest::RequestBuilder> {
    let header = basic_auth(
        ctx.require_secret("patchmonApiKey")?,
        ctx.require_secret("patchmonApiSecret")?,
    );
    Ok(http::get(rt, ctx, "/api/v1/gethomepage/stats", &[])?
        .timeout(http::timeout(STATS_REQUEST_TIMEOUT_SECS))
        .header("Authorization", header))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let value = http::send_json(ctx, stats_request(rt, ctx)?, "PatchMon stats").await?;
    map_stats(ctx, &value).map(|_| ())
}

pub async fn stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<PatchMonStats> {
    let value = http::send_json(ctx, stats_request(rt, ctx)?, "PatchMon stats").await?;
    map_stats(ctx, &value)
}

pub fn map_stats(ctx: &IntegrationContext, value: &Value) -> HomarrResult<PatchMonStats> {
    let invalid = || parse_error(ctx, "PatchMon stats", "Invalid PatchMon stats response");
    let number = |key: &str| value.get(key).and_then(Value::as_f64).ok_or_else(invalid);
    let mut os_distribution = Vec::new();
    if let Some(entries) = value.get("os_distribution").filter(|v| !v.is_null()) {
        for entry in entries.as_array().ok_or_else(invalid)? {
            os_distribution.push(PatchMonOsDistributionEntry {
                name: entry
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?
                    .to_string(),
                count: entry
                    .get("count")
                    .and_then(Value::as_f64)
                    .ok_or_else(invalid)?,
                os_type: entry
                    .get("os_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                os_version: entry
                    .get("os_version")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    // Largest groups first (stable for equal counts, like `toSorted`).
    os_distribution.sort_by(|a, b| b.count.total_cmp(&a.count));
    Ok(PatchMonStats {
        total_hosts: number("total_hosts")?,
        hosts_needing_updates: number("hosts_needing_updates")?,
        security_updates: number("security_updates")?,
        up_to_date_hosts: number("up_to_date_hosts")?,
        hosts_with_security_updates: number("hosts_with_security_updates")?,
        recent_updates_24h: number("recent_updates_24h")?,
        total_outdated_packages: number("total_outdated_packages")?,
        total_repos: number("total_repos")?,
        last_updated: value
            .get("last_updated")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?
            .to_string(),
        os_distribution,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "p".into(),
            name: "PatchMon".into(),
            url: "http://patchmon".into(),
            external_url: None,
            kind: IntegrationKind::Patchmon,
            secrets: HashMap::new(),
        }
    }

    fn payload() -> Value {
        json!({
            "total_hosts": 10, "hosts_needing_updates": 4, "security_updates": 2, "up_to_date_hosts": 6,
            "hosts_with_security_updates": 1, "recent_updates_24h": 3, "total_outdated_packages": 40,
            "total_repos": 5, "last_updated": "2026-01-01T00:00:00Z",
            "os_distribution": [
                {"name":"Debian","count":2,"os_type":"linux"},
                {"name":"Ubuntu","count":7,"os_version":"24.04"}
            ]
        })
    }

    #[test]
    fn maps_and_sorts_os_distribution() {
        let stats = map_stats(&ctx(), &payload()).unwrap();
        assert_eq!(stats.total_hosts, 10.0);
        assert_eq!(stats.os_distribution[0].name, "Ubuntu");
        assert_eq!(
            stats.os_distribution[0].os_version.as_deref(),
            Some("24.04")
        );
        assert_eq!(stats.os_distribution[1].os_type.as_deref(), Some("linux"));
    }

    #[test]
    fn defaults_missing_distribution_and_rejects_missing_fields() {
        let mut value = payload();
        value.as_object_mut().unwrap().remove("os_distribution");
        assert!(map_stats(&ctx(), &value)
            .unwrap()
            .os_distribution
            .is_empty());
        value.as_object_mut().unwrap().remove("total_repos");
        assert!(map_stats(&ctx(), &value).is_err());
    }
}
