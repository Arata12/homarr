use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Bytes, Count},
    StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{nonneg, num};

pub struct SyncthingRelay;

const METRICS: &[MetricDef] = &[
    m("numActiveSessions", "Active sessions", Count),
    m("numConnections", "Connections", Count),
    m("bytesProxied", "Bytes proxied", Bytes),
];

#[async_trait]
impl StatsProvider for SyncthingRelay {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/status", &Vec::new()).await?;
        parsed(client.ctx, "Syncthing relay status", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        (
            "numActiveSessions",
            num(nonneg(response, &["numActiveSessions"])?),
        ),
        (
            "numConnections",
            num(nonneg(response, &["numConnections"])?),
        ),
        ("bytesProxied", num(nonneg(response, &["bytesProxied"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_status() {
        let out = parse(&json!({
            "numActiveSessions": 1, "numConnections": 2, "bytesProxied": 3000000, "goVersion": "x"
        }))
        .unwrap();
        assert_eq!(out["bytesProxied"], StatsValue::Number(3_000_000.0));
        assert!(
            parse(&json!({ "numActiveSessions": -1, "numConnections": 2, "bytesProxied": 0 }))
                .is_err()
        );
    }
}
