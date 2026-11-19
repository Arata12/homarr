use async_trait::async_trait;
use homarr_common::HomarrResult;
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, Body, StatsClient};
use crate::groups::stats::json::{array, boolean, count, len, num};

pub struct Unmanic;

const METRICS: &[MetricDef] = &[
    m("activeWorkers", "Active workers", Count),
    m("totalWorkers", "Total workers", Count),
    m("recordsTotal", "Pending records", Count),
];

#[async_trait]
impl StatsProvider for Unmanic {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let none = Vec::new();
        let workers = client.get("/unmanic/api/v2/workers/status", &none).await?;
        let pending = client
            .send(
                Method::POST,
                "/unmanic/api/v2/pending/tasks",
                &none,
                Body::Json(json!({
                    "start": 0,
                    "length": 0,
                    "search_value": "",
                    "status": "all",
                    "order_by": "priority",
                    "order_direction": "desc",
                    "library_ids": [],
                })),
            )
            .await?;
        parsed(client.ctx, "Unmanic statistics", parse(&workers, &pending))
    }
}

fn parse(workers: &Value, pending: &Value) -> Result<StatsValues, String> {
    let workers = array(workers, &["workers_status"])?;
    let mut active = 0;
    for worker in workers {
        if !boolean(worker, &["idle"])? {
            active += 1;
        }
    }
    Ok(values([
        ("activeWorkers", len(active)),
        ("totalWorkers", len(workers.len())),
        ("recordsTotal", num(count(pending, &["recordsTotal"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn counts_busy_workers() {
        let out = parse(
            &json!({ "workers_status": [{ "idle": false }, { "idle": true }, { "idle": false }] }),
            &json!({ "recordsTotal": 14 }),
        )
        .unwrap();
        assert_eq!(out["activeWorkers"], StatsValue::Number(2.0));
        assert_eq!(out["totalWorkers"], StatsValue::Number(3.0));
        assert_eq!(out["recordsTotal"], StatsValue::Number(14.0));
    }
}
