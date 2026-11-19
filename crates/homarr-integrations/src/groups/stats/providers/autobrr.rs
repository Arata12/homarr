use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, len, num};

pub struct Autobrr;

const METRICS: &[MetricDef] = &[
    m("approvedPushes", "Approved pushes", Count),
    m("rejectedPushes", "Rejected pushes", Count),
    m("filters", "Filters", Count),
    m("indexers", "Indexers", Count),
];

#[async_trait]
impl StatsProvider for Autobrr {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let no_headers = Vec::new();
        let (stats, filters, indexers) = tokio::try_join!(
            client.get("/api/release/stats", &no_headers),
            client.get("/api/filters", &no_headers),
            client.get("/api/release/indexers", &no_headers),
        )?;
        parsed(
            client.ctx,
            "autobrr statistics",
            parse(&stats, &filters, &indexers),
        )
    }
}

fn parse(stats: &Value, filters: &Value, indexers: &Value) -> Result<StatsValues, String> {
    let filters = filters.as_array().ok_or("filters must be an array")?;
    let indexers = indexers.as_array().ok_or("indexers must be an array")?;
    Ok(values([
        (
            "approvedPushes",
            num(count(stats, &["push_approved_count"])?),
        ),
        (
            "rejectedPushes",
            num(count(stats, &["push_rejected_count"])?),
        ),
        ("filters", len(filters.len())),
        ("indexers", len(indexers.len())),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_counts() {
        let out = parse(
            &json!({ "push_approved_count": 5, "push_rejected_count": 2 }),
            &json!([{}, {}]),
            &json!([{}]),
        )
        .unwrap();
        assert_eq!(out["approvedPushes"], StatsValue::Number(5.0));
        assert_eq!(out["filters"], StatsValue::Number(2.0));
        assert_eq!(out["indexers"], StatsValue::Number(1.0));
    }

    #[test]
    fn rejects_negative_counts() {
        assert!(parse(
            &json!({ "push_approved_count": -1, "push_rejected_count": 2 }),
            &json!([]),
            &json!([])
        )
        .is_err());
    }
}
