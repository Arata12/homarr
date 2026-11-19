//! Small helpers shared by the home & misc integrations.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use homarr_common::{HomarrError, HomarrResult};
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::error::{check_status, parse_error, request_error};
use crate::types::IntegrationContext;

/// Port of `Integration.createUrl`: joins `base` (trailing slashes removed) and `path`, then sets the query
/// parameters (`None` values are skipped).
pub fn create_url(base: &str, path: &str, query: &[(&str, Option<String>)]) -> HomarrResult<Url> {
    let joined = format!("{}{}", base.trim_end_matches('/'), path);
    let mut url = Url::parse(&joined).map_err(|err| {
        HomarrError::BadRequest(format!("Invalid integration URL '{joined}': {err}"))
    })?;
    let pairs: Vec<(&str, &String)> = query
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|value| (*key, value)))
        .collect();
    if !pairs.is_empty() {
        let mut serializer = url.query_pairs_mut();
        for (key, value) in pairs {
            serializer.append_pair(key, value);
        }
    }
    Ok(url)
}

/// `Integration.url`.
pub fn url(
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<Url> {
    create_url(&ctx.url, path, query)
}

/// `Integration.externalUrl`: falls back to the integration URL.
pub fn external_url(ctx: &IntegrationContext, path: &str) -> HomarrResult<Url> {
    let base = ctx
        .external_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .unwrap_or(&ctx.url);
    create_url(base, path, &[])
}

pub fn q(value: impl ToString) -> Option<String> {
    Some(value.to_string())
}

pub fn basic_auth(username: &str, password: &str) -> String {
    format!("Basic {}", B64.encode(format!("{username}:{password}")))
}

/// Sends the request; transport failures become descriptive upstream errors. The status is not checked.
pub async fn send_raw(ctx: &IntegrationContext, request: RequestBuilder) -> HomarrResult<Response> {
    request.send().await.map_err(|err| request_error(ctx, &err))
}

/// Sends the request and requires a success status (TS `ResponseError` otherwise).
pub async fn send(ctx: &IntegrationContext, request: RequestBuilder) -> HomarrResult<Response> {
    check_status(ctx, send_raw(ctx, request).await?)
}

/// Reads the JSON body into `T`; parse failures name `what`.
pub async fn read_json<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    response: Response,
) -> HomarrResult<T> {
    let text = response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    parse_json(ctx, what, &text)
}

pub fn parse_json<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    text: &str,
) -> HomarrResult<T> {
    serde_json::from_str(text).map_err(|err| parse_error(ctx, what, err))
}

pub fn from_value<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    value: Value,
) -> HomarrResult<T> {
    serde_json::from_value(value).map_err(|err| parse_error(ctx, what, err))
}

/// GET + success status + JSON.
pub async fn get_json<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    http: &homarr_core::HttpClientFactory,
    what: &str,
    url: Url,
    headers: &[(&str, String)],
) -> HomarrResult<T> {
    let mut request = http.standard().get(url);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    read_json(ctx, what, send(ctx, request).await?).await
}

pub fn secret_opt<'a>(ctx: &'a IntegrationContext, key: &str) -> Option<&'a str> {
    ctx.secret(key).filter(|value| !value.is_empty())
}

/// Typed access to the JSON input of `IntegrationGroup::integration_data` operations.
pub fn input_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

pub fn input_u64(input: &Value, key: &str) -> Option<u64> {
    input.get(key).and_then(Value::as_u64)
}

pub fn unknown_operation(ctx: &IntegrationContext, operation: &str) -> HomarrError {
    HomarrError::BadRequest(format!(
        "{} does not support the operation '{operation}'",
        ctx.kind.as_str()
    ))
}

pub fn to_json<T: serde::Serialize>(value: &T) -> HomarrResult<Value> {
    serde_json::to_value(value).map_err(|err| {
        HomarrError::Internal(format!("Failed to serialize integration data: {err}"))
    })
}
