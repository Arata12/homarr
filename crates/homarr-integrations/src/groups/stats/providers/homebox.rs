use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Count, Text},
    StatsValue, StatsValues,
};
use crate::groups::stats::client::{parsed, Body, Headers, StatsClient};
use crate::groups::stats::json::{count, nonneg, num, opt, text};

pub struct Homebox;

const METRICS: &[MetricDef] = &[
    m("items", "Items", Count),
    m("locations", "Locations", Count),
    m("labels", "Labels", Count),
    m("itemsWithWarranty", "Items with warranty", Count),
    m("totalValue", "Total value", Text),
    m("users", "Users", Count),
];

#[async_trait]
impl StatsProvider for Homebox {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = authenticate(client).await?;
        let stats = client.get("/api/v1/groups/statistics", &headers).await?;
        let group = client.get("/api/v1/groups", &headers).await?;
        parsed(client.ctx, "Homebox statistics", parse(&stats, &group))
    }
}

async fn authenticate(client: &StatsClient<'_>) -> HomarrResult<Headers> {
    if let Some(api_key) = client.secret("apiKey") {
        return Ok(vec![("Authorization", format!("Bearer {api_key}"))]);
    }
    let failed = || HomarrError::Upstream("Homebox authentication failed".to_string());
    let username = client.secret("username").ok_or_else(failed)?;
    let password = client.secret("password").ok_or_else(failed)?;
    let response = client
        .send(
            Method::POST,
            "/api/v1/users/login",
            &Vec::new(),
            Body::Form(vec![
                ("username", username.to_string()),
                ("password", password.to_string()),
            ]),
        )
        .await
        .map_err(|_| failed())?;
    let token = parse_login(&response).map_err(|_| failed())?;
    Ok(vec![("Authorization", token)])
}

fn parse_login(response: &Value) -> Result<String, String> {
    let token = text(response, &["token"])?;
    if token.is_empty() {
        return Err("empty token".to_string());
    }
    Ok(token.to_string())
}

fn parse(stats: &Value, group: &Value) -> Result<StatsValues, String> {
    let labels = match (opt(stats, &["totalLabels"]), opt(stats, &["totalTags"])) {
        (None, None) => return Err("Expected a labels or tags count".to_string()),
        (Some(value), _) | (None, Some(value)) => {
            crate::groups::stats::json::as_count(value, "totalLabels")?
        }
    };
    let currency = text(group, &["currency"])?;
    if currency.is_empty() {
        return Err("'currency' must not be empty".to_string());
    }
    let price = nonneg(stats, &["totalItemPrice"])?;
    Ok(values([
        ("items", num(count(stats, &["totalItems"])?)),
        ("locations", num(count(stats, &["totalLocations"])?)),
        ("labels", num(labels)),
        (
            "itemsWithWarranty",
            num(count(stats, &["totalWithWarranty"])?),
        ),
        (
            "totalValue",
            StatsValue::Text(format!("{currency} {price}")),
        ),
        ("users", num(count(stats, &["totalUsers"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn stats(extra: Value) -> Value {
        let mut base = json!({
            "totalItems": 10, "totalLocations": 2, "totalWithWarranty": 3,
            "totalItemPrice": 1234.5, "totalUsers": 1
        });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    #[test]
    fn maps_tags_or_labels_and_formats_value() {
        let group = json!({ "currency": "EUR" });
        let out = parse(&stats(json!({ "totalTags": 4 })), &group).unwrap();
        assert_eq!(out["labels"], StatsValue::Number(4.0));
        assert_eq!(out["totalValue"], StatsValue::Text("EUR 1234.5".into()));
        let out = parse(&stats(json!({ "totalLabels": 7 })), &group).unwrap();
        assert_eq!(out["labels"], StatsValue::Number(7.0));
    }

    #[test]
    fn requires_labels_or_tags() {
        assert!(parse(&stats(json!({})), &json!({ "currency": "EUR" })).is_err());
    }

    #[test]
    fn login_requires_non_empty_token() {
        assert_eq!(
            parse_login(&json!({ "token": "abc" })),
            Ok("abc".to_string())
        );
        assert!(parse_login(&json!({ "token": "" })).is_err());
        assert!(parse_login(&json!({})).is_err());
    }
}
