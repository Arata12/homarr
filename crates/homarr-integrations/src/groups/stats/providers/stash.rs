use async_trait::async_trait;
use homarr_common::HomarrResult;
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Bytes, Count, Seconds},
    StatsValues,
};
use crate::groups::stats::client::{parsed, Body, StatsClient};
use crate::groups::stats::json::{count, nonneg, num};

pub struct Stash;

const METRICS: &[MetricDef] = &[
    m("scenes", "Scenes", Count),
    m("sceneBytes", "Scene size", Bytes),
    m("sceneDuration", "Scene duration", Seconds),
    m("images", "Images", Count),
    m("imageBytes", "Image size", Bytes),
    m("galleries", "Galleries", Count),
    m("performers", "Performers", Count),
    m("studios", "Studios", Count),
    m("tags", "Tags", Count),
];

const QUERY: &str = "query {
  stats {
    scene_count
    scenes_size
    scenes_duration
    image_count
    images_size
    gallery_count
    performer_count
    studio_count
    tag_count
  }
}";

#[async_trait]
impl StatsProvider for Stash {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = vec![("ApiKey", client.require_secret("apiKey")?.to_string())];
        let response = client
            .send(
                Method::POST,
                "/graphql",
                &headers,
                Body::Json(json!({ "query": QUERY })),
            )
            .await?;
        parsed(client.ctx, "Stash statistics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    if response.get("errors").is_some() {
        return Err("the GraphQL response contains errors".to_string());
    }
    let stats = |key: &'static str| ["data", "stats", key];
    Ok(values([
        ("scenes", num(count(response, &stats("scene_count"))?)),
        ("sceneBytes", num(nonneg(response, &stats("scenes_size"))?)),
        (
            "sceneDuration",
            num(nonneg(response, &stats("scenes_duration"))?),
        ),
        ("images", num(count(response, &stats("image_count"))?)),
        ("imageBytes", num(nonneg(response, &stats("images_size"))?)),
        ("galleries", num(count(response, &stats("gallery_count"))?)),
        (
            "performers",
            num(count(response, &stats("performer_count"))?),
        ),
        ("studios", num(count(response, &stats("studio_count"))?)),
        ("tags", num(count(response, &stats("tag_count"))?)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::stats::StatsValue;

    fn response() -> Value {
        json!({ "data": { "stats": {
            "scene_count": 10, "scenes_size": 1024.5, "scenes_duration": 3600,
            "image_count": 5, "images_size": 10, "gallery_count": 2,
            "performer_count": 3, "studio_count": 4, "tag_count": 6
        } } })
    }

    #[test]
    fn maps_stats_and_rejects_graphql_errors() {
        let out = parse(&response()).unwrap();
        assert_eq!(out["sceneBytes"], StatsValue::Number(1024.5));
        assert_eq!(out["tags"], StatsValue::Number(6.0));
        let mut with_errors = response();
        with_errors["errors"] = json!([{ "message": "boom" }]);
        assert!(parse(&with_errors).is_err());
    }
}
