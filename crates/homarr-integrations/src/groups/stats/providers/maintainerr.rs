use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Bytes, Count},
    StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, nonneg, num};

pub struct Maintainerr;

const METRICS: &[MetricDef] = &[
    m("itemsHandled", "Items handled", Count),
    m("episodesHandled", "Episodes handled", Count),
    m("moviesHandled", "Movies handled", Count),
    m("reclaimable", "Reclaimable storage", Bytes),
];

#[async_trait]
impl StatsProvider for Maintainerr {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/api/storage-metrics", &Vec::new()).await?;
        parsed(client.ctx, "Maintainerr storage metrics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        (
            "itemsHandled",
            num(count(response, &["cleanupTotals", "itemsHandled"])?),
        ),
        (
            "episodesHandled",
            num(count(response, &["cleanupTotals", "episodesHandled"])?),
        ),
        (
            "moviesHandled",
            num(count(response, &["cleanupTotals", "moviesHandled"])?),
        ),
        (
            "reclaimable",
            num(nonneg(response, &["collectionSummary", "activeSizeBytes"])?),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_nested_totals() {
        let out = parse(&json!({
            "cleanupTotals": { "itemsHandled": 5, "episodesHandled": 3, "moviesHandled": 2 },
            "collectionSummary": { "activeSizeBytes": 1024 }
        }))
        .unwrap();
        assert_eq!(out["reclaimable"], StatsValue::Number(1024.0));
        assert!(parse(&json!({ "cleanupTotals": {} })).is_err());
    }
}
