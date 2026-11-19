use async_trait::async_trait;
use homarr_common::HomarrResult;
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, Body, Headers, StatsClient};
use crate::groups::stats::json::{count, len, num};

pub struct Komga;

const METRICS: &[MetricDef] = &[
    m("libraries", "Libraries", Count),
    m("series", "Series", Count),
    m("books", "Books", Count),
];

#[async_trait]
impl StatsProvider for Komga {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers: Headers = vec![("Accept", "application/json".to_string())];
        let (libraries, series, books) = tokio::try_join!(
            client.get("/api/v1/libraries", &headers),
            client.send(
                Method::POST,
                "/api/v1/series/list",
                &headers,
                Body::Json(json!({}))
            ),
            client.send(
                Method::POST,
                "/api/v1/books/list",
                &headers,
                Body::Json(json!({}))
            ),
        )?;
        parsed(
            client.ctx,
            "Komga statistics",
            parse(&libraries, &series, &books),
        )
    }
}

fn parse(libraries: &Value, series: &Value, books: &Value) -> Result<StatsValues, String> {
    let libraries = libraries.as_array().ok_or("libraries must be an array")?;
    let mut available = 0;
    for library in libraries {
        if !library.is_object() {
            return Err("library must be an object".to_string());
        }
        match library.get("unavailable") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => available += 1,
            Some(Value::Bool(true)) => {}
            Some(_) => return Err("'unavailable' must be a boolean".to_string()),
        }
    }
    Ok(values([
        ("libraries", len(available)),
        ("series", num(count(series, &["totalElements"])?)),
        ("books", num(count(books, &["totalElements"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn skips_unavailable_libraries() {
        let out = parse(
            &json!([{ "id": "1" }, { "id": "2", "unavailable": true }, { "unavailable": false }]),
            &json!({ "totalElements": 12 }),
            &json!({ "totalElements": 340 }),
        )
        .unwrap();
        assert_eq!(out["libraries"], StatsValue::Number(2.0));
        assert_eq!(out["series"], StatsValue::Number(12.0));
        assert_eq!(out["books"], StatsValue::Number(340.0));
    }
}
