use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{nonneg, num};

pub struct Jellystat;

const METRICS: &[MetricDef] = &[
    m("songs", "Song plays (30 days)", Count),
    m("movies", "Movie plays (30 days)", Count),
    m("episodes", "Episode plays (30 days)", Count),
    m("other", "Other plays (30 days)", Count),
];

#[async_trait]
impl StatsProvider for Jellystat {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = vec![("X-API-Token", client.require_secret("apiKey")?.to_string())];
        let response = client
            .get("/stats/getViewsByLibraryType?days=30", &headers)
            .await?;
        parsed(client.ctx, "Jellystat views", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("songs", num(nonneg(response, &["Audio"])?)),
        ("movies", num(nonneg(response, &["Movie"])?)),
        ("episodes", num(nonneg(response, &["Series"])?)),
        ("other", num(nonneg(response, &["Other"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_library_types() {
        let out = parse(&json!({ "Audio": 1, "Movie": 2, "Series": 3, "Other": 4 })).unwrap();
        assert_eq!(out["songs"], StatsValue::Number(1.0));
        assert_eq!(out["episodes"], StatsValue::Number(3.0));
        assert!(parse(&json!({ "Audio": 1 })).is_err());
    }
}
