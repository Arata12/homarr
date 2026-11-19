//! Small request helpers shared by the systems integrations. All traffic goes through `rt.http`.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::{Method, RequestBuilder, Response};
use serde_json::Value;
use url::Url;

use crate::error::{check_status, parse_error, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// Builds `<base><path>?<query>`; `path` must start with `/`. Empty query values are kept as given.
pub fn build_url(base: &str, path: &str, query: &[(&str, &str)]) -> HomarrResult<Url> {
    let joined = format!("{}{}", base.trim_end_matches('/'), path);
    let mut url = Url::parse(&joined).map_err(|err| {
        HomarrError::BadRequest(format!("Invalid integration URL '{joined}': {err}"))
    })?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    Ok(url)
}

pub fn url(ctx: &IntegrationContext, path: &str, query: &[(&str, &str)]) -> HomarrResult<Url> {
    build_url(ctx.base_url(), path, query)
}

pub fn request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: Method,
    path: &str,
    query: &[(&str, &str)],
) -> HomarrResult<RequestBuilder> {
    Ok(rt.http.standard().request(method, url(ctx, path, query)?))
}

pub fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, &str)],
) -> HomarrResult<RequestBuilder> {
    request(rt, ctx, Method::GET, path, query)
}

/// Sends the request; transport failures and non-success statuses become descriptive errors.
pub async fn send(ctx: &IntegrationContext, req: RequestBuilder) -> HomarrResult<Response> {
    let response = req.send().await.map_err(|err| request_error(ctx, &err))?;
    check_status(ctx, response)
}

pub async fn send_text(ctx: &IntegrationContext, req: RequestBuilder) -> HomarrResult<String> {
    let response = send(ctx, req).await?;
    response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))
}

pub async fn send_json(
    ctx: &IntegrationContext,
    req: RequestBuilder,
    what: &str,
) -> HomarrResult<Value> {
    let text = send_text(ctx, req).await?;
    serde_json::from_str(&text).map_err(|err| parse_error(ctx, what, err))
}

pub fn timeout(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

pub fn basic_auth(user: &str, password: &str) -> String {
    format!("Basic {}", B64.encode(format!("{user}:{password}")))
}

/// JSON number or numeric string.
pub fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
        _ => None,
    }
}

pub fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn f64_of(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(as_f64)
}

/// Collects `Set-Cookie` headers as `name=value` pairs joined by `; `.
pub fn cookie_header(response: &Response) -> Option<String> {
    let cookies: Vec<String> = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(str::to_string)
        .collect();
    (!cookies.is_empty()).then(|| cookies.join("; "))
}

pub fn invalid(ctx: &IntegrationContext, what: &str) -> HomarrError {
    parse_error(ctx, what, "unexpected response shape")
}
