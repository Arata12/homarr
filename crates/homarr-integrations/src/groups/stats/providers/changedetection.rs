use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{boolean, len, object};

pub struct Changedetection;

const METRICS: &[MetricDef] = &[
    m("diffsDetected", "Diffs detected", Count),
    m("totalObserved", "Total observed", Count),
];

#[async_trait]
impl StatsProvider for Changedetection {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = vec![("x-api-key", client.require_secret("apiKey")?.to_string())];
        let response = client.get("/api/v1/watch", &headers).await?;
        parsed(client.ctx, "changedetection watches", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let watches = object(response, &[])?;
    let mut diffs = 0;
    for watch in watches.values() {
        let last_changed = last_changed(watch)?;
        let viewed = boolean(watch, &["viewed"])?;
        if last_changed > 0.0 && !viewed {
            diffs += 1;
        }
    }
    Ok(values([
        ("diffsDetected", len(diffs)),
        ("totalObserved", len(watches.len())),
    ]))
}

/// `last_changed` is any finite number (zod `z.number().finite()`).
fn last_changed(watch: &Value) -> Result<f64, String> {
    watch
        .get("last_changed")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| "'last_changed' must be a number".to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn counts_unviewed_changes() {
        let out = parse(&json!({
            "a": { "last_changed": 100, "viewed": false },
            "b": { "last_changed": 100, "viewed": true },
            "c": { "last_changed": 0, "viewed": false }
        }))
        .unwrap();
        assert_eq!(out["diffsDetected"], StatsValue::Number(1.0));
        assert_eq!(out["totalObserved"], StatsValue::Number(3.0));
    }
}
