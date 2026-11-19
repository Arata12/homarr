//! Maps transport, HTTP-status and parse failures to `HomarrError`, mirroring `base/errors/*` of the TS
//! implementation (`IntegrationRequestError`, `IntegrationResponseError`, `IntegrationParseError`,
//! `IntegrationUnknownError`).
//!
//! Everything returned here is `HomarrError::Upstream` (or `BadRequest` for configuration problems), never
//! `Unauthorized`/`Forbidden`: those would be read as the *caller* being logged out. The message names the
//! integration and the failure class so users can act on it. Secrets, URLs with credentials and response bodies
//! are never included.
#![allow(dead_code)] // Helpers are used by the group implementations as they get ported.

use std::error::Error as _;

use homarr_common::HomarrError;
use reqwest::StatusCode;

use crate::types::IntegrationContext;

/// Failure class of a request that did not produce a response (TS `RequestError` types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFailure {
    Certificate,
    Timeout,
    Connection,
    Redirect,
    Other,
}

fn source_chain(err: &reqwest::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    text.to_lowercase()
}

pub fn classify_request_error(err: &reqwest::Error) -> RequestFailure {
    let chain = source_chain(err);
    if chain.contains("certificate")
        || chain.contains("unknownissuer")
        || chain.contains("self signed")
    {
        RequestFailure::Certificate
    } else if err.is_timeout() {
        RequestFailure::Timeout
    } else if err.is_redirect() {
        RequestFailure::Redirect
    } else if err.is_connect()
        || chain.contains("dns error")
        || chain.contains("connection refused")
    {
        RequestFailure::Connection
    } else {
        RequestFailure::Other
    }
}

/// The request never got a response (TS `IntegrationRequestError`).
pub fn request_error(ctx: &IntegrationContext, err: &reqwest::Error) -> HomarrError {
    let name = &ctx.name;
    let message = match classify_request_error(err) {
        RequestFailure::Certificate => format!(
            "Request to {name} failed: the TLS certificate is not trusted. Add the certificate to the trusted certificates or use http"
        ),
        RequestFailure::Timeout => format!("Request to {name} timed out"),
        RequestFailure::Connection => format!(
            "Unable to connect to {name} at {}. Check the URL, the port and that the service is running",
            ctx.base_url()
        ),
        RequestFailure::Redirect => format!("Request to {name} was redirected too many times"),
        // The URL may carry credentials in its query string, so it is stripped from the message.
        RequestFailure::Other => {
            let mut text = err.to_string();
            if let Some(url) = err.url() {
                text = text.replace(url.as_str(), "<url>");
            }
            format!("Request to {name} failed: {text}")
        }
    };
    HomarrError::Upstream(message)
}

/// The response status did not indicate success (TS `IntegrationResponseError`).
pub fn status_error(ctx: &IntegrationContext, status: StatusCode) -> HomarrError {
    let name = &ctx.name;
    let code = status.as_u16();
    let message = match code {
        401 => format!("{name} rejected the credentials (HTTP 401). Check the configured API key, username or password"),
        403 => format!("{name} denied access (HTTP 403). Check the permissions of the configured credentials"),
        404 => format!("{name} returned HTTP 404. Check the URL (including any base path) and the service version"),
        429 => format!("{name} is rate limiting requests (HTTP 429)"),
        500..=599 => format!("{name} reported a server error (HTTP {code})"),
        _ => format!("{name} returned HTTP {code}"),
    };
    HomarrError::Upstream(message)
}

/// The response could not be parsed into the expected shape (TS `IntegrationParseError`).
pub fn parse_error(
    ctx: &IntegrationContext,
    what: &str,
    err: impl std::fmt::Display,
) -> HomarrError {
    HomarrError::Upstream(format!(
        "Failed to parse {what} from {}: {err}. The service version may be unsupported",
        ctx.name
    ))
}

/// Sanitize reqwest body-read/JSON-decode errors so URLs containing tokens are never leaked.
pub fn response_body_error(
    ctx: &IntegrationContext,
    what: &str,
    err: reqwest::Error,
) -> HomarrError {
    parse_error(ctx, what, err.without_url())
}

/// Anything else that went wrong while executing an integration method (TS `IntegrationUnknownError`).
pub fn unknown_error(ctx: &IntegrationContext, err: impl std::fmt::Display) -> HomarrError {
    HomarrError::Upstream(format!(
        "An unknown error occurred while executing an integration method of {}: {err}",
        ctx.name
    ))
}

/// A required capability is not implemented for the kind.
pub fn unsupported(ctx: &IntegrationContext, capability: &str) -> HomarrError {
    HomarrError::BadRequest(format!(
        "{} does not support {capability}",
        ctx.kind.as_str()
    ))
}

/// Returns `Ok(response)` for success statuses, otherwise `status_error`.
pub fn check_status(
    ctx: &IntegrationContext,
    response: reqwest::Response,
) -> Result<reqwest::Response, HomarrError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(status_error(ctx, response.status()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "i".into(),
            name: "Sonarr".into(),
            url: "http://sonarr.local:8989".into(),
            external_url: None,
            kind: IntegrationKind::Sonarr,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn status_errors_are_descriptive() {
        let err = status_error(&ctx(), StatusCode::UNAUTHORIZED).to_string();
        assert!(err.contains("Sonarr") && err.contains("credentials"));
        let err = status_error(&ctx(), StatusCode::BAD_GATEWAY).to_string();
        assert!(err.contains("502"));
    }

    #[test]
    fn unsupported_names_kind_and_capability() {
        let err = unsupported(&ctx(), "calendar").to_string();
        assert_eq!(err, "sonarr does not support calendar");
    }
}
