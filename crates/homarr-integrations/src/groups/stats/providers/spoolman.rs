use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Count, Grams},
    StatsValue, StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{as_nonneg, boolean, count, len, opt};

pub struct Spoolman;

const METRICS: &[MetricDef] = &[
    m("spools", "Spools", Count),
    m("remainingWeight", "Remaining weight", Grams),
];

#[async_trait]
impl StatsProvider for Spoolman {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/api/v1/spool", &Vec::new()).await?;
        parsed(client.ctx, "Spoolman spools", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let list = response.as_array().ok_or("spools must be an array")?;
    let mut active = Vec::new();
    for spool in list {
        count(spool, &["id"])?;
        let weight = opt(spool, &["remaining_weight"])
            .map(|value| as_nonneg(value, "remaining_weight"))
            .transpose()?;
        if !boolean(spool, &["archived"])? {
            active.push(weight);
        }
    }
    // One spool without a known weight makes the total unknown.
    let remaining = active
        .iter()
        .copied()
        .sum::<Option<f64>>()
        .map_or(StatsValue::Null, StatsValue::Number);
    Ok(values([
        ("spools", len(active.len())),
        ("remainingWeight", remaining),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sums_remaining_weight_of_active_spools() {
        let out = parse(&json!([
            { "id": 1, "remaining_weight": 100.5, "archived": false },
            { "id": 2, "remaining_weight": 50, "archived": false },
            { "id": 3, "remaining_weight": null, "archived": true }
        ]))
        .unwrap();
        assert_eq!(out["spools"], StatsValue::Number(2.0));
        assert_eq!(out["remainingWeight"], StatsValue::Number(150.5));
    }

    #[test]
    fn unknown_weight_makes_total_null() {
        let out = parse(&json!([
            { "id": 1, "remaining_weight": 10, "archived": false },
            { "id": 2, "archived": false }
        ]))
        .unwrap();
        assert_eq!(out["remainingWeight"], StatsValue::Null);
    }
}
