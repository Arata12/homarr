use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Count, Text},
    StatsValue, StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, num, opt};

pub struct Fileflows;

const METRICS: &[MetricDef] = &[
    m("queue", "Queued", Count),
    m("processing", "Processing", Count),
    m("processed", "Processed", Count),
    m("time", "Processing time", Text),
];

#[async_trait]
impl StatsProvider for Fileflows {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/api/status", &Vec::new()).await?;
        parsed(client.ctx, "FileFlows status", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let time = match opt(response, &["time"]) {
        None => StatsValue::Null,
        Some(value) => {
            StatsValue::Text(value.as_str().ok_or("'time' must be a string")?.to_string())
        }
    };
    Ok(values([
        ("queue", num(count(response, &["queue"])?)),
        ("processing", num(count(response, &["processing"])?)),
        ("processed", num(count(response, &["processed"])?)),
        ("time", time),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn missing_time_is_null() {
        let out = parse(&json!({ "queue": 1, "processing": 2, "processed": 3 })).unwrap();
        assert_eq!(out["time"], StatsValue::Null);
        let out =
            parse(&json!({ "queue": 1, "processing": 2, "processed": 3, "time": "1h" })).unwrap();
        assert_eq!(out["time"], StatsValue::Text("1h".into()));
    }
}
