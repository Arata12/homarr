use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{count, num};

pub struct Mealie;

const METRICS: &[MetricDef] = &[
    m("recipes", "Recipes", Count),
    m("users", "Users", Count),
    m("categories", "Categories", Count),
    m("tags", "Tags", Count),
];

#[async_trait]
impl StatsProvider for Mealie {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let key = client.require_secret("apiKey")?;
        let headers = vec![("Authorization", format!("Bearer {key}"))];
        let response = client.get("/api/households/statistics", &headers).await?;
        parsed(client.ctx, "Mealie statistics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("recipes", num(count(response, &["totalRecipes"])?)),
        ("users", num(count(response, &["totalUsers"])?)),
        ("categories", num(count(response, &["totalCategories"])?)),
        ("tags", num(count(response, &["totalTags"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_totals() {
        let out = parse(&json!({
            "totalRecipes": 10, "totalUsers": 2, "totalCategories": 3, "totalTags": 4
        }))
        .unwrap();
        assert_eq!(out["recipes"], StatsValue::Number(10.0));
        assert!(parse(&json!({ "totalRecipes": 1.5 })).is_err());
    }
}
