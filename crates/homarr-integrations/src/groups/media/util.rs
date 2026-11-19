//! Shared HTTP/URL helpers of the media group.

use homarr_common::{HomarrError, HomarrResult};
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{check_status, parse_error, request_error};
use crate::types::IntegrationContext;

/// `Integration.url`: base url (without trailing slash) + path + query (`None` values are skipped).
pub fn build_url(base: &str, path: &str, query: &[(&str, Option<String>)]) -> HomarrResult<Url> {
    let joined = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut url = Url::parse(&joined)
        .map_err(|err| HomarrError::BadRequest(format!("Invalid integration URL: {err}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            if let Some(value) = value {
                pairs.append_pair(key, value);
            }
        }
    }
    if url.query() == Some("") {
        url.set_query(None);
    }
    Ok(url)
}

/// URL against the internal integration url.
pub fn url(
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<Url> {
    build_url(&ctx.url, path, query)
}

/// `Integration.externalUrl`: the url shown to users (falls back to the internal one).
pub fn external_url(ctx: &IntegrationContext, path: &str) -> String {
    let base = ctx
        .external_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&ctx.url);
    build_url(base, path, &[]).map_or_else(
        |_| format!("{}{path}", base.trim_end_matches('/')),
        |url| url.to_string(),
    )
}

pub fn q(value: impl ToString) -> Option<String> {
    Some(value.to_string())
}

/// Sends a request and maps transport / status failures to descriptive errors.
pub async fn send(ctx: &IntegrationContext, request: RequestBuilder) -> HomarrResult<Response> {
    let response = request
        .send()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    check_status(ctx, response)
}

pub async fn send_text(ctx: &IntegrationContext, request: RequestBuilder) -> HomarrResult<String> {
    let response = send(ctx, request).await?;
    response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))
}

pub async fn send_json<T: DeserializeOwned>(
    ctx: &IntegrationContext,
    what: &str,
    request: RequestBuilder,
) -> HomarrResult<T> {
    let text = send_text(ctx, request).await?;
    serde_json::from_str(&text).map_err(|err| parse_error(ctx, what, err))
}

/// ISO string like JavaScript's `Date.toISOString()`.
pub fn iso(date: chrono::DateTime<chrono::Utc>) -> String {
    date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls_like_the_integration_base_class() {
        let url = build_url(
            "http://sonarr:8989/",
            "/api/v3/calendar",
            &[("page", q(1)), ("skip", None), ("flag", q(true))],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://sonarr:8989/api/v3/calendar?page=1&flag=true"
        );
        let url = build_url("http://sonarr:8989", "/api", &[]).unwrap();
        assert_eq!(url.as_str(), "http://sonarr:8989/api");
    }
}
