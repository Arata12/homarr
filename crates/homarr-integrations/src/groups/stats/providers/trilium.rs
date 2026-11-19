use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Bytes, Count, Text},
    StatsValue, StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, nonneg, num, text};

pub struct Trilium;

const METRICS: &[MetricDef] = &[
    m("version", "Version", Text),
    m("notesCount", "Notes", Count),
    m("dbSize", "Database size", Bytes),
];

#[async_trait]
impl StatsProvider for Trilium {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client
            .get("/etapi/metrics?format=json", &Vec::new())
            .await?;
        parsed(client.ctx, "Trilium metrics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let version = text(response, &["version", "app"])?;
    if version.is_empty() {
        return Err("'version.app' must not be empty".to_string());
    }
    Ok(values([
        ("version", StatsValue::Text(version.to_string())),
        (
            "notesCount",
            num(count(response, &["database", "activeNotes"])?),
        ),
        (
            "dbSize",
            num(nonneg(response, &["statistics", "databaseSizeBytes"])?),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_metrics() {
        let out = parse(&json!({
            "version": { "app": "0.63.7" },
            "database": { "activeNotes": 120 },
            "statistics": { "databaseSizeBytes": 4096 }
        }))
        .unwrap();
        assert_eq!(out["version"], StatsValue::Text("0.63.7".into()));
        assert_eq!(out["notesCount"], StatsValue::Number(120.0));
        assert!(parse(&json!({ "version": { "app": "" } })).is_err());
    }
}
