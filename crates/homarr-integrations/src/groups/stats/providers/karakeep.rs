use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{nonneg, num};

pub struct Karakeep;

const METRICS: &[MetricDef] = &[
    m("bookmarks", "Bookmarks", Count),
    m("favorites", "Favorites", Count),
    m("archived", "Archived", Count),
    m("highlights", "Highlights", Count),
    m("lists", "Lists", Count),
    m("tags", "Tags", Count),
];

#[async_trait]
impl StatsProvider for Karakeep {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let key = client.require_secret("apiKey")?;
        let headers = vec![("Authorization", format!("Bearer {key}"))];
        let response = client.get("/api/v1/users/me/stats", &headers).await?;
        parsed(client.ctx, "Karakeep statistics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("bookmarks", num(nonneg(response, &["numBookmarks"])?)),
        ("favorites", num(nonneg(response, &["numFavorites"])?)),
        ("archived", num(nonneg(response, &["numArchived"])?)),
        ("highlights", num(nonneg(response, &["numHighlights"])?)),
        ("lists", num(nonneg(response, &["numLists"])?)),
        ("tags", num(nonneg(response, &["numTags"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_counters() {
        let out = parse(&json!({
            "numBookmarks": 9, "numFavorites": 1, "numArchived": 2,
            "numHighlights": 3, "numLists": 4, "numTags": 5, "extra": true
        }))
        .unwrap();
        assert_eq!(out["bookmarks"], StatsValue::Number(9.0));
        assert_eq!(out["tags"], StatsValue::Number(5.0));
    }
}
