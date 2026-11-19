use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, len, text};

pub struct Healthchecks;

const METRICS: &[MetricDef] = &[
    m("checksUp", "Up checks", Count),
    m("checksDown", "Down checks", Count),
    m("checksGrace", "Checks in grace", Count),
    m("checksNew", "New checks", Count),
];

#[async_trait]
impl StatsProvider for Healthchecks {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = vec![("X-Api-Key", client.require_secret("apiKey")?.to_string())];
        let response = client.get("/api/v3/checks/", &headers).await?;
        parsed(client.ctx, "Healthchecks checks", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let (mut up, mut down, mut grace, mut new) = (0, 0, 0, 0);
    for check in array(response, &["checks"])? {
        match text(check, &["status"])? {
            "up" => up += 1,
            "down" => down += 1,
            "grace" => grace += 1,
            "new" => new += 1,
            "paused" => {}
            other => return Err(format!("unknown check status '{other}'")),
        }
    }
    Ok(values([
        ("checksUp", len(up)),
        ("checksDown", len(down)),
        ("checksGrace", len(grace)),
        ("checksNew", len(new)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn counts_by_status() {
        let out = parse(&json!({ "checks": [
            { "status": "up" }, { "status": "up" }, { "status": "down" },
            { "status": "grace" }, { "status": "new" }, { "status": "paused" }
        ] }))
        .unwrap();
        assert_eq!(out["checksUp"], StatsValue::Number(2.0));
        assert_eq!(out["checksDown"], StatsValue::Number(1.0));
        assert_eq!(out["checksGrace"], StatsValue::Number(1.0));
        assert_eq!(out["checksNew"], StatsValue::Number(1.0));
    }

    #[test]
    fn rejects_unknown_status() {
        assert!(parse(&json!({ "checks": [{ "status": "boom" }] })).is_err());
    }
}
