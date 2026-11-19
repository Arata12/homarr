use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{boolean, len, object};

pub struct Scrutiny;

const METRICS: &[MetricDef] = &[
    m("passed", "Passed devices", Count),
    m("failed", "Failed devices", Count),
    m("unknown", "Unknown devices", Count),
];

const PASSED: i64 = 0;
const FAILED_BOTH: i64 = 3;

#[async_trait]
impl StatsProvider for Scrutiny {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let none = Vec::new();
        let (summary, settings) = tokio::try_join!(
            client.get("/api/summary", &none),
            client.get("/api/settings", &none),
        )?;
        parsed(client.ctx, "Scrutiny summary", parse(&summary, &settings))
    }
}

fn int(value: &Value, path: &[&str]) -> Result<i64, String> {
    crate::groups::stats::json::at(value, path)?
        .as_i64()
        .ok_or_else(|| format!("'{}' must be an integer", path.join(".")))
}

fn parse(summary: &Value, settings: &Value) -> Result<StatsValues, String> {
    let threshold = int(settings, &["settings", "metrics", "status_threshold"])?;
    let mut statuses = Vec::new();
    for entry in object(summary, &["data", "summary"])?.values() {
        let archived = boolean(entry, &["device", "archived"])?;
        let status = int(entry, &["device", "device_status"])?;
        if !archived {
            statuses.push(status);
        }
    }
    let failed = statuses
        .iter()
        .filter(|&&status| {
            if threshold == FAILED_BOTH {
                status > PASSED && status <= FAILED_BOTH
            } else {
                status == threshold || status == FAILED_BOTH
            }
        })
        .count();
    let unknown = statuses
        .iter()
        .filter(|&&status| !(PASSED..=FAILED_BOTH).contains(&status))
        .count();
    Ok(values([
        ("passed", len(statuses.len() - failed - unknown)),
        ("failed", len(failed)),
        ("unknown", len(unknown)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    fn summary() -> Value {
        let device = |status: i64, archived: bool| json!({ "device": { "archived": archived, "device_status": status } });
        json!({ "data": { "summary": {
            "a": device(0, false), "b": device(1, false), "c": device(2, false),
            "d": device(3, false), "e": device(-1, false), "f": device(2, true)
        } } })
    }

    #[test]
    fn threshold_both_counts_any_failure() {
        let out = parse(
            &summary(),
            &json!({ "settings": { "metrics": { "status_threshold": 3 } } }),
        )
        .unwrap();
        assert_eq!(out["failed"], StatsValue::Number(3.0));
        assert_eq!(out["unknown"], StatsValue::Number(1.0));
        assert_eq!(out["passed"], StatsValue::Number(1.0));
    }

    #[test]
    fn specific_threshold_counts_matching_and_both() {
        let out = parse(
            &summary(),
            &json!({ "settings": { "metrics": { "status_threshold": 1 } } }),
        )
        .unwrap();
        assert_eq!(out["failed"], StatsValue::Number(2.0));
        assert_eq!(out["passed"], StatsValue::Number(2.0));
    }
}
