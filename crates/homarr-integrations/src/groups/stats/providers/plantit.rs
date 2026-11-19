use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, Body, Headers, StatsClient};
use crate::groups::stats::json::{count, num, text};

pub struct Plantit;

const METRICS: &[MetricDef] = &[
    m("plants", "Plants", Count),
    m("species", "Species", Count),
    m("photos", "Photos", Count),
    m("events", "Events", Count),
];

#[async_trait]
impl StatsProvider for Plantit {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = authenticate(client).await?;
        let response = client.get("/api/stats", &headers).await?;
        parsed(client.ctx, "Plant-it statistics", parse(&response))
    }
}

async fn authenticate(client: &StatsClient<'_>) -> HomarrResult<Headers> {
    let failed = || HomarrError::Upstream("Plant-it authentication failed".to_string());
    let username = client.secret("username").ok_or_else(failed)?;
    let password = client.secret("password").ok_or_else(failed)?;
    let response = client
        .send(
            Method::POST,
            "/api/authentication/login",
            &Vec::new(),
            Body::Json(json!({ "username": username, "password": password })),
        )
        .await
        .map_err(|_| failed())?;
    let token = parse_login(&response).map_err(|_| failed())?;
    Ok(vec![("Authorization", format!("Bearer {token}"))])
}

fn parse_login(response: &Value) -> Result<&str, String> {
    let token = text(response, &["jwt", "value"])?;
    if token.is_empty() {
        return Err("empty token".to_string());
    }
    Ok(token)
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    Ok(values([
        ("plants", num(count(response, &["plantCount"])?)),
        ("species", num(count(response, &["botanicalInfoCount"])?)),
        ("photos", num(count(response, &["imageCount"])?)),
        ("events", num(count(response, &["diaryEntryCount"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn maps_counts_and_login() {
        let out = parse(&json!({
            "diaryEntryCount": 4, "plantCount": 1, "imageCount": 3, "botanicalInfoCount": 2
        }))
        .unwrap();
        assert_eq!(out["plants"], StatsValue::Number(1.0));
        assert_eq!(out["events"], StatsValue::Number(4.0));
        assert_eq!(parse_login(&json!({ "jwt": { "value": "t" } })), Ok("t"));
        assert!(parse_login(&json!({ "jwt": { "value": "" } })).is_err());
    }
}
