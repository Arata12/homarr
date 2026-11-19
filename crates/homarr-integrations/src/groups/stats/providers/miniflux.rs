use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{as_count, num, object};

pub struct Miniflux;

const METRICS: &[MetricDef] = &[m("read", "Read", Count), m("unread", "Unread", Count)];

#[async_trait]
impl StatsProvider for Miniflux {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = vec![("X-Auth-Token", client.require_secret("apiKey")?.to_string())];
        let response = client.get("/v1/feeds/counters", &headers).await?;
        parsed(client.ctx, "Miniflux counters", parse(&response))
    }
}

fn sum(response: &Value, key: &str) -> Result<f64, String> {
    object(response, &[key])?
        .values()
        .try_fold(0.0, |total, value| Ok(total + as_count(value, key)?))
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("read", num(sum(response, "reads")?)),
        ("unread", num(sum(response, "unreads")?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn sums_feed_counters() {
        let out = parse(&json!({ "reads": { "1": 5, "2": 7 }, "unreads": { "1": 1 } })).unwrap();
        assert_eq!(out["read"], StatsValue::Number(12.0));
        assert_eq!(out["unread"], StatsValue::Number(1.0));
    }
}
