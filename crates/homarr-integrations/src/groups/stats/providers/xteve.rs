use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, Body, StatsClient};
use crate::groups::stats::json::{as_nonneg, num, opt};

pub struct Xteve;

const METRICS: &[MetricDef] = &[
    m("streamsAll", "All streams", Count),
    m("streamsActive", "Active streams", Count),
    m("streamsXepg", "XEPG streams", Count),
];

#[async_trait]
impl StatsProvider for Xteve {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let token = login_token(client).await?;
        let mut payload = json!({ "cmd": "status" });
        if let Some(token) = token {
            payload["token"] = Value::String(token);
        }
        let response = client
            .send(Method::POST, "/api/", &Vec::new(), Body::Json(payload))
            .await?;
        parsed(client.ctx, "xTeVe status", parse(&response))
    }
}

/// `None` when no credentials are saved (xTeVe without authentication).
async fn login_token(client: &StatsClient<'_>) -> HomarrResult<Option<String>> {
    let failed = || HomarrError::Upstream("xTeVe authentication failed".to_string());
    let (username, password) = match (client.secret("username"), client.secret("password")) {
        (None, None) => return Ok(None),
        (Some(username), Some(password)) => (username, password),
        _ => return Err(failed()),
    };
    let response = client
        .send(
            Method::POST,
            "/api/",
            &Vec::new(),
            Body::Json(json!({ "cmd": "login", "username": username, "password": password })),
        )
        .await
        .map_err(|_| failed())?;
    parse_login(&response).map(Some).map_err(|_| failed())
}

fn parse_login(response: &Value) -> Result<String, String> {
    if response.get("status") != Some(&Value::Bool(true)) {
        return Err("login was not accepted".to_string());
    }
    match response.get("token").and_then(Value::as_str) {
        Some(token) if !token.is_empty() => Ok(token.to_string()),
        _ => Err("missing token".to_string()),
    }
}

/// xTeVe omits zero-valued fields because its Go response uses `omitempty`.
fn parse(response: &Value) -> Result<StatsValues, String> {
    if response.get("status") != Some(&Value::Bool(true)) {
        return Err("expected status true".to_string());
    }
    let field = |key: &str| match opt(response, &[key]) {
        Some(value) => as_nonneg(value, key),
        None => Ok(0.0),
    };
    Ok(values([
        ("streamsAll", num(field("streams.all")?)),
        ("streamsActive", num(field("streams.active")?)),
        ("streamsXepg", num(field("streams.xepg")?)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn omitted_fields_default_to_zero() {
        let out = parse(&json!({ "status": true, "streams.all": 12 })).unwrap();
        assert_eq!(out["streamsAll"], StatsValue::Number(12.0));
        assert_eq!(out["streamsActive"], StatsValue::Number(0.0));
        assert_eq!(out["streamsXepg"], StatsValue::Number(0.0));
    }

    #[test]
    fn rejects_unsuccessful_responses() {
        assert!(parse(&json!({ "status": false })).is_err());
        assert!(parse_login(&json!({ "status": true })).is_err());
        assert_eq!(
            parse_login(&json!({ "status": true, "token": "t" })),
            Ok("t".to_string())
        );
    }
}
