//! Small request helpers shared by the integrations of this group. All traffic goes through
//! `rt.http` and failures are mapped with `crate::error`.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use homarr_common::HomarrResult;
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::clients::build_request_url;
use crate::error::{check_status, parse_error, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// `base_url + path` (the path may carry a query string) plus extra query pairs.
pub fn url(ctx: &IntegrationContext, path: &str, query: &[(&str, &str)]) -> HomarrResult<Url> {
    let map = query
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    build_request_url(ctx.base_url(), path, &map)
}

/// Like [`url`] but keeps the order of the query pairs (needed when a server cares about it).
pub fn url_ordered(
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, &str)],
) -> HomarrResult<Url> {
    let mut url = build_request_url(ctx.base_url(), path, &Default::default())?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query {
            pairs.append_pair(k, v);
        }
    }
    Ok(url)
}

pub fn get(rt: &IntegrationRuntime, url: Url) -> RequestBuilder {
    rt.http.standard().get(url)
}

pub fn post(rt: &IntegrationRuntime, url: Url) -> RequestBuilder {
    rt.http.standard().post(url)
}

pub fn basic_auth_value(user: &str, pass: &str) -> String {
    format!("Basic {}", B64.encode(format!("{user}:{pass}")))
}

/// Sends the request and requires a success status.
pub async fn send(ctx: &IntegrationContext, req: RequestBuilder) -> HomarrResult<Response> {
    let resp = req.send().await.map_err(|err| request_error(ctx, &err))?;
    check_status(ctx, resp)
}

/// Sends the request without status check (for callers inspecting 401/404/409 themselves).
pub async fn send_raw(ctx: &IntegrationContext, req: RequestBuilder) -> HomarrResult<Response> {
    req.send().await.map_err(|err| request_error(ctx, &err))
}

pub async fn read_json(ctx: &IntegrationContext, resp: Response) -> HomarrResult<Value> {
    resp.json::<Value>()
        .await
        .map_err(|err| crate::error::response_body_error(ctx, "the response body", err))
}

pub async fn read_text(ctx: &IntegrationContext, resp: Response) -> HomarrResult<String> {
    resp.text()
        .await
        .map_err(|err| crate::error::response_body_error(ctx, "the response body", err))
}

pub async fn send_json(ctx: &IntegrationContext, req: RequestBuilder) -> HomarrResult<Value> {
    let resp = send(ctx, req).await?;
    read_json(ctx, resp).await
}

pub fn from_value<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    value: Value,
) -> HomarrResult<T> {
    serde_json::from_value(value).map_err(|err| parse_error(ctx, what, err))
}

/// `name=value` pairs of every `Set-Cookie` header, joined for a `Cookie` header.
pub fn cookie_header(resp: &Response) -> Option<String> {
    let cookies: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|cookie| cookie.split(';').next())
        .map(str::to_string)
        .filter(|cookie| !cookie.is_empty())
        .collect();
    if cookies.is_empty() {
        None
    } else {
        Some(cookies.join("; "))
    }
}
