use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValue, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, count, num};

pub struct Tandoor;

const METRICS: &[MetricDef] = &[
    m("users", "Users in first space", Count),
    m("recipes", "Recipes in first space", Count),
    m("keywords", "Keywords", Count),
];

#[async_trait]
impl StatsProvider for Tandoor {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let key = client.require_secret("apiKey")?;
        let headers = vec![("Authorization", format!("Bearer {key}"))];
        let (spaces, keywords) = tokio::try_join!(
            client.get("/api/space/", &headers),
            client.get("/api/keyword/?page=1&page_size=1", &headers),
        )?;
        parsed(client.ctx, "Tandoor statistics", parse(&spaces, &keywords))
    }
}

fn parse(spaces: &Value, keywords: &Value) -> Result<StatsValues, String> {
    let list = if let Some(list) = spaces.as_array() {
        list
    } else {
        array(spaces, &["results"])?
    };
    for space in list {
        count(space, &["user_count"])?;
        count(space, &["recipe_count"])?;
    }
    let first = |key: &str| match list.first() {
        Some(space) => count(space, &[key]).map(num),
        None => Ok(StatsValue::Null),
    };
    Ok(values([
        ("users", first("user_count")?),
        ("recipes", first("recipe_count")?),
        ("keywords", num(count(keywords, &["count"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn uses_first_space_in_either_envelope() {
        let spaces = json!([{ "user_count": 3, "recipe_count": 40 }, { "user_count": 1, "recipe_count": 1 }]);
        let out = parse(&spaces, &json!({ "count": 12 })).unwrap();
        assert_eq!(out["users"], StatsValue::Number(3.0));
        assert_eq!(out["keywords"], StatsValue::Number(12.0));
        let out = parse(&json!({ "results": spaces }), &json!({ "count": 0 })).unwrap();
        assert_eq!(out["recipes"], StatsValue::Number(40.0));
    }

    #[test]
    fn no_space_is_null() {
        let out = parse(&json!([]), &json!({ "count": 2 })).unwrap();
        assert_eq!(out["users"], StatsValue::Null);
        assert_eq!(out["recipes"], StatsValue::Null);
    }
}
