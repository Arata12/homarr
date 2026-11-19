//! What's up Docker (`wud/wud-integration.ts`): container update statistics.

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, basic_auth};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const CONTAINERS_REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WudContainerUpdate {
    pub id: String,
    pub name: String,
    pub current_version: Option<String>,
    pub new_version: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WudStats {
    pub total_containers: usize,
    pub updates_available: usize,
    pub updates: Vec<WudContainerUpdate>,
}

fn auth_header(ctx: &IntegrationContext) -> Option<String> {
    if let Some(token) = ctx.secret("personalAccessToken") {
        return Some(format!("Bearer {token}"));
    }
    Some(basic_auth(ctx.secret("username")?, ctx.secret("password")?))
}

fn containers_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<reqwest::RequestBuilder> {
    let mut req = http::get(rt, ctx, "/api/containers", &[])?
        .timeout(http::timeout(CONTAINERS_REQUEST_TIMEOUT_SECS));
    if let Some(header) = auth_header(ctx) {
        req = req.header("Authorization", header);
    }
    Ok(req)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let value = http::send_json(ctx, containers_request(rt, ctx)?, "WUD containers").await?;
    validate_containers(ctx, &value)
}

pub async fn stats(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<WudStats> {
    let value = http::send_json(ctx, containers_request(rt, ctx)?, "WUD containers").await?;
    map_stats(ctx, &value)
}

fn validate_containers(ctx: &IntegrationContext, value: &Value) -> HomarrResult<()> {
    map_stats(ctx, value).map(|_| ())
}

fn opt_string(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Validates the container list shape (`id`, `name`, `updateAvailable` required) and maps it.
pub fn map_stats(ctx: &IntegrationContext, value: &Value) -> HomarrResult<WudStats> {
    let invalid = || parse_error(ctx, "WUD containers", "Invalid WUD containers response");
    let containers = value.as_array().ok_or_else(invalid)?;
    let mut updates = Vec::new();
    for container in containers {
        let id = container
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let name = container
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let update_available = container
            .get("updateAvailable")
            .and_then(Value::as_bool)
            .ok_or_else(invalid)?;
        if !update_available {
            continue;
        }
        updates.push(WudContainerUpdate {
            id: id.to_string(),
            name: opt_string(container, "/displayName").unwrap_or_else(|| name.to_string()),
            current_version: opt_string(container, "/image/tag/value"),
            new_version: opt_string(container, "/updateKind/remoteValue")
                .or_else(|| opt_string(container, "/result/tag")),
            link: opt_string(container, "/link"),
        });
    }
    Ok(WudStats {
        total_containers: containers.len(),
        updates_available: updates.len(),
        updates,
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
            id: "w".into(),
            name: "WUD".into(),
            url: "http://wud".into(),
            external_url: None,
            kind: IntegrationKind::Wud,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn maps_updates() {
        let containers = json!([
            {"id":"1","name":"web","displayName":"Web","updateAvailable":true,"link":"https://x","image":{"tag":{"value":"1.0"}},"updateKind":{"remoteValue":"1.1"}},
            {"id":"2","name":"db","updateAvailable":true,"result":{"tag":"15"}},
            {"id":"3","name":"cache","updateAvailable":false}
        ]);
        let stats = map_stats(&ctx(), &containers).unwrap();
        assert_eq!(stats.total_containers, 3);
        assert_eq!(stats.updates_available, 2);
        assert_eq!(stats.updates[0].name, "Web");
        assert_eq!(stats.updates[0].current_version.as_deref(), Some("1.0"));
        assert_eq!(stats.updates[0].new_version.as_deref(), Some("1.1"));
        assert_eq!(stats.updates[1].name, "db");
        assert_eq!(stats.updates[1].new_version.as_deref(), Some("15"));
        assert_eq!(stats.updates[1].link, None);
    }

    #[test]
    fn rejects_invalid_payloads() {
        assert!(map_stats(&ctx(), &json!({"a":1})).is_err());
        assert!(map_stats(&ctx(), &json!([{"id":"1","name":"x"}])).is_err());
    }
}
