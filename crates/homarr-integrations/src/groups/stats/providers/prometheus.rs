use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, len, text};

pub struct Prometheus;

const METRICS: &[MetricDef] = &[
    m("total", "Targets", Count),
    m("up", "Up", Count),
    m("down", "Down", Count),
];

#[async_trait]
impl StatsProvider for Prometheus {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let mut headers = Vec::new();
        if let (Some(username), Some(password)) =
            (client.secret("username"), client.secret("password"))
        {
            let encoded = STANDARD.encode(format!("{username}:{password}"));
            headers.push(("Authorization", format!("Basic {encoded}")));
        }
        let response = client.get("/api/v1/targets", &headers).await?;
        parsed(client.ctx, "Prometheus targets", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    if text(response, &["status"])? != "success" {
        return Err("expected status 'success'".to_string());
    }
    array(response, &["data", "droppedTargets"])?;
    let targets = array(response, &["data", "activeTargets"])?;
    let (mut up, mut down) = (0, 0);
    for target in targets {
        match text(target, &["health"])? {
            "up" => up += 1,
            "down" => down += 1,
            _ => {}
        }
    }
    Ok(values([
        ("total", len(targets.len())),
        ("up", len(up)),
        ("down", len(down)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn counts_target_health() {
        let out = parse(&json!({
            "status": "success",
            "data": {
                "activeTargets": [{ "health": "up" }, { "health": "down" }, { "health": "unknown" }],
                "droppedTargets": []
            }
        }))
        .unwrap();
        assert_eq!(out["total"], StatsValue::Number(3.0));
        assert_eq!(out["up"], StatsValue::Number(1.0));
        assert_eq!(out["down"], StatsValue::Number(1.0));
    }

    #[test]
    fn rejects_error_status() {
        assert!(parse(&json!({ "status": "error", "data": {} })).is_err());
    }
}
