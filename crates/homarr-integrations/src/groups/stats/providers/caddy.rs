use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, len, num};

pub struct Caddy;

const METRICS: &[MetricDef] = &[
    m("upstreams", "Upstreams", Count),
    m("requests", "Active requests", Count),
    m("requestsFailed", "Failed requests", Count),
];

#[async_trait]
impl StatsProvider for Caddy {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/reverse_proxy/upstreams", &Vec::new()).await?;
        parsed(client.ctx, "Caddy upstreams", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let upstreams = response.as_array().ok_or("upstreams must be an array")?;
    let mut requests = 0.0;
    let mut failed = 0.0;
    for upstream in upstreams {
        requests += count(upstream, &["num_requests"])?;
        failed += count(upstream, &["fails"])?;
    }
    Ok(values([
        ("upstreams", len(upstreams.len())),
        ("requests", num(requests)),
        ("requestsFailed", num(failed)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn sums_requests_and_failures() {
        let out = parse(&json!([
            { "address": "a", "num_requests": 3, "fails": 1 },
            { "address": "b", "num_requests": 4, "fails": 0 }
        ]))
        .unwrap();
        assert_eq!(out["upstreams"], StatsValue::Number(2.0));
        assert_eq!(out["requests"], StatsValue::Number(7.0));
        assert_eq!(out["requestsFailed"], StatsValue::Number(1.0));
    }
}
