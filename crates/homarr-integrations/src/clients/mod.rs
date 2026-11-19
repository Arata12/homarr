pub mod dns_hole;
pub mod downloads;
pub mod health;
pub mod media;
pub mod mock;
pub mod network_misc;
pub mod servarr;
pub mod stats;

use std::collections::HashMap;

use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use reqwest::Method;
use serde_json::Value;
use url::Url;

use crate::http_auth::{resolve_declared_http_auth, ResolvedHttpAuth};
use crate::types::IntegrationContext;

pub(crate) fn build_request_url(
    base: &str,
    path: &str,
    query: &HashMap<String, String>,
) -> HomarrResult<Url> {
    let joined = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut url = Url::parse(&joined).map_err(|err| {
        HomarrError::BadRequest(format!("Invalid integration URL '{joined}': {err}"))
    })?;
    {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query {
            pairs.append_pair(k, v);
        }
    }
    Ok(url)
}

pub(crate) async fn get_json(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Value> {
    let auth = resolve_declared_http_auth(ctx)?;
    get_json_with_auth(http, ctx, path, &auth).await
}

pub(crate) async fn get_json_with_auth(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    path: &str,
    auth: &ResolvedHttpAuth,
) -> HomarrResult<Value> {
    request_json_with_auth(http, ctx, Method::GET, path, None, auth).await
}

pub(crate) async fn request_json_with_auth(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: &ResolvedHttpAuth,
) -> HomarrResult<Value> {
    let url = build_request_url(ctx.base_url(), path, &auth.query)?;
    let mut req = http.standard().request(method, url);
    for (k, v) in &auth.headers {
        req = req.header(k, v);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req
        .send()
        .await
        .map_err(|err| HomarrError::Upstream(format!("{} request failed: {err}", ctx.name)))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(HomarrError::Upstream(format!(
            "{} returned HTTP {}",
            ctx.name,
            status.as_u16()
        )));
    }
    let text = resp.text().await.map_err(|err| {
        HomarrError::Upstream(format!("Failed reading response from {}: {err}", ctx.name))
    })?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

pub(crate) async fn simple_get_test(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<()> {
    let auth = resolve_declared_http_auth(ctx)?;
    let url = build_request_url(ctx.base_url(), path, &auth.query)?;
    let mut req = http.standard().get(url);
    for (k, v) in &auth.headers {
        req = req.header(k, v);
    }
    let resp = req
        .send()
        .await
        .map_err(|err| HomarrError::Upstream(format!("Connection failed: {err}")))?;
    if !resp.status().is_success() {
        return Err(HomarrError::Upstream(format!(
            "HTTP {}",
            resp.status().as_u16()
        )));
    }
    Ok(())
}
