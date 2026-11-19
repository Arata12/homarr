use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, num};

pub struct Netdata;

const METRICS: &[MetricDef] = &[
    m("warnings", "Warnings", Count),
    m("criticals", "Criticals", Count),
];

#[async_trait]
impl StatsProvider for Netdata {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/api/v1/info", &Vec::new()).await?;
        parsed(client.ctx, "Netdata info", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("warnings", num(count(response, &["alarms", "warning"])?)),
        ("criticals", num(count(response, &["alarms", "critical"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_alarms() {
        let out =
            parse(&json!({ "alarms": { "warning": 2, "critical": 1, "normal": 9 } })).unwrap();
        assert_eq!(out["warnings"], StatsValue::Number(2.0));
        assert_eq!(out["criticals"], StatsValue::Number(1.0));
    }
}
