use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::num;

pub struct Netalertx;

const METRICS: &[MetricDef] = &[
    m("total", "Total devices", Count),
    m("connected", "Connected devices", Count),
    m("newDevices", "New devices", Count),
    m("downAlerts", "Down alerts", Count),
];

#[async_trait]
impl StatsProvider for Netalertx {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let key = client.require_secret("apiKey")?;
        let headers = vec![("Authorization", format!("Bearer {key}"))];
        let response = client.get("/devices/totals", &headers).await?;
        parsed(client.ctx, "NetAlertX totals", parse(&response))
    }
}

/// A total is a non-negative integer or a string of digits.
fn total(value: &Value) -> Result<f64, String> {
    match value {
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => text
            .parse::<f64>()
            .map_err(|_| "total is not a number".to_string()),
        other => crate::groups::stats::json::as_count(other, "total"),
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let totals = response.as_array().ok_or("totals must be an array")?;
    if totals.len() < 5 {
        return Err("expected at least five totals".to_string());
    }
    let totals = totals.iter().map(total).collect::<Result<Vec<_>, _>>()?;
    Ok(values([
        ("total", num(totals[0])),
        ("connected", num(totals[1])),
        ("newDevices", num(totals[3])),
        ("downAlerts", num(totals[4])),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_positional_totals_with_string_numbers() {
        let out = parse(&json!([10, "8", 1, 2, "3"])).unwrap();
        assert_eq!(out["total"], StatsValue::Number(10.0));
        assert_eq!(out["connected"], StatsValue::Number(8.0));
        assert_eq!(out["newDevices"], StatsValue::Number(2.0));
        assert_eq!(out["downAlerts"], StatsValue::Number(3.0));
    }

    #[test]
    fn rejects_short_or_invalid_totals() {
        assert!(parse(&json!([1, 2, 3])).is_err());
        assert!(parse(&json!([1, 2, 3, 4, "x"])).is_err());
    }
}
