use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, boolean, len};

pub struct Gatus;

const METRICS: &[MetricDef] = &[
    m("up", "Up", Count),
    m("down", "Down", Count),
    m("unknown", "Unknown", Count),
    m("total", "Total", Count),
];

#[async_trait]
impl StatsProvider for Gatus {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client
            .get("/api/v1/endpoints/statuses", &Vec::new())
            .await?;
        parsed(client.ctx, "Gatus endpoint statuses", parse(&response))
    }
}

/// The API returns either a list of endpoints or a record keyed by endpoint key.
fn parse(response: &Value) -> Result<StatsValues, String> {
    let endpoints: Vec<&Value> = match response {
        Value::Array(list) => list.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => return Err("expected a list or record of endpoints".to_string()),
    };
    let mut up = 0;
    let mut down = 0;
    for endpoint in &endpoints {
        let results = array(endpoint, &["results"])?;
        for result in results {
            boolean(result, &["success"])?;
        }
        match results
            .last()
            .and_then(|r| r.get("success"))
            .and_then(Value::as_bool)
        {
            Some(true) => up += 1,
            Some(false) => down += 1,
            None => {}
        }
    }
    Ok(values([
        ("up", len(up)),
        ("down", len(down)),
        ("unknown", len(endpoints.len() - up - down)),
        ("total", len(endpoints.len())),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn uses_last_result_of_each_endpoint() {
        let out = parse(&json!([
            { "results": [{ "success": false }, { "success": true }] },
            { "results": [{ "success": true }, { "success": false }] },
            { "results": [] }
        ]))
        .unwrap();
        assert_eq!(out["up"], StatsValue::Number(1.0));
        assert_eq!(out["down"], StatsValue::Number(1.0));
        assert_eq!(out["unknown"], StatsValue::Number(1.0));
        assert_eq!(out["total"], StatsValue::Number(3.0));
    }

    #[test]
    fn accepts_record_shape() {
        let out = parse(&json!({ "a": { "results": [{ "success": true }] } })).unwrap();
        assert_eq!(out["up"], StatsValue::Number(1.0));
    }
}
