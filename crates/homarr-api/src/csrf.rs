//! CSRF protection for cookie-authenticated requests.
//!
//! Unsafe methods carrying the session cookie must present an `Origin` (or `Referer`) whose
//! host[:port] matches the request host. API-key / Bearer requests and cookie-less requests are
//! exempt.

use axum::{
    extract::{Request, State},
    http::{
        header::{AUTHORIZATION, COOKIE, ORIGIN, REFERER},
        HeaderMap, Method, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use homarr_auth::{session_cookie_name, API_KEY_HEADER_NAME};
use url::Url;

use crate::state::AppState;

fn first_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn authority_of_url(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    Some(match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    })
    .map(|a| a.to_ascii_lowercase())
}

pub fn has_bearer_authorization(headers: &HeaderMap) -> bool {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.len() >= 7 && v[..7].eq_ignore_ascii_case("bearer "))
}

pub fn has_session_cookie(headers: &HeaderMap, cookie_name: &str) -> bool {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .any(|part| {
            part.trim()
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == cookie_name)
        })
}

pub fn has_valid_same_origin(headers: &HeaderMap, base_url: Option<&str>) -> bool {
    let Some(source_url) = first_header(headers, ORIGIN.as_str())
        .or_else(|| first_header(headers, REFERER.as_str()))
        .and_then(|raw| Url::parse(raw).ok())
    else {
        return false;
    };
    let Some(source_authority) = authority_of_url(&source_url) else {
        return false;
    };

    if let Some(raw_host) =
        first_header(headers, "x-forwarded-host").or_else(|| first_header(headers, "host"))
    {
        if let Some(req_authority) = Url::parse(&format!("{}://{raw_host}", source_url.scheme()))
            .ok()
            .and_then(|u| authority_of_url(&u))
        {
            if req_authority == source_authority {
                return true;
            }
        }
    }

    if let Some(base) = base_url.and_then(|b| Url::parse(b).ok()) {
        if base.scheme().eq_ignore_ascii_case(source_url.scheme())
            && authority_of_url(&base).as_deref() == Some(source_authority.as_str())
        {
            return true;
        }
    }

    false
}

/// Returns true when the request must be rejected as a cross-origin cookie/proxy-authenticated write.
fn is_csrf_violation(
    method: &Method,
    headers: &HeaderMap,
    cookie_name: &str,
    trusted_proxy_header: Option<&str>,
    base_url: Option<&str>,
) -> bool {
    if matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return false;
    }
    if headers.contains_key(API_KEY_HEADER_NAME) || has_bearer_authorization(headers) {
        return false;
    }
    let has_cookie = has_session_cookie(headers, cookie_name);
    let has_proxy_auth = trusted_proxy_header.is_some_and(|h| first_header(headers, h).is_some());
    if !has_cookie && !has_proxy_auth {
        return false;
    }

    !has_valid_same_origin(headers, base_url)
}

pub async fn csrf_middleware(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let cookie_name = session_cookie_name(&state.config.auth_cookie_prefix);
    if is_csrf_violation(
        req.method(),
        req.headers(),
        &cookie_name,
        state.config.auth_trusted_proxy_header.as_deref(),
        state.config.base_url.as_deref(),
    ) {
        return (
            StatusCode::FORBIDDEN,
            "Cross-origin request rejected (missing or mismatching Origin)",
        )
            .into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    const C: &str = "homarr.session-token";

    #[test]
    fn csrf_rules() {
        let cookie = ("cookie", "homarr.session-token=abc");
        let post = Method::POST;
        // same origin allowed (Origin and Referer fallback)
        assert!(!is_csrf_violation(
            &post,
            &headers(&[cookie, ("host", "h:7575"), ("origin", "http://h:7575")]),
            C,
            None,
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[cookie, ("host", "h:7575"), ("referer", "http://h:7575/x")]),
            C,
            None,
            None
        ));
        // forwarded host wins over host and default ports normalize cleanly
        assert!(!is_csrf_violation(
            &post,
            &headers(&[
                cookie,
                ("host", "internal:1"),
                ("x-forwarded-host", "app.example"),
                ("origin", "https://app.example")
            ]),
            C,
            None,
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[
                cookie,
                ("host", "internal:1"),
                ("x-forwarded-host", "app.example:443"),
                ("origin", "https://app.example")
            ]),
            C,
            None,
            None
        ));
        // BASE_URL fallback allows proxies forwarding internal Host without X-Forwarded-Host
        assert!(!is_csrf_violation(
            &post,
            &headers(&[
                cookie,
                ("host", "127.0.0.1:7575"),
                ("origin", "https://dash.example")
            ]),
            C,
            None,
            Some("https://dash.example")
        ));
        // cross origin, null origin and missing origin denied
        assert!(is_csrf_violation(
            &post,
            &headers(&[cookie, ("host", "h:7575"), ("origin", "http://evil.com")]),
            C,
            None,
            None
        ));
        assert!(is_csrf_violation(
            &post,
            &headers(&[cookie, ("host", "h:7575"), ("origin", "null")]),
            C,
            None,
            None
        ));
        assert!(is_csrf_violation(
            &Method::DELETE,
            &headers(&[cookie, ("host", "h:7575")]),
            C,
            None,
            None
        ));
        // Authorization: Basic does NOT exempt cookie sessions from CSRF checks
        assert!(is_csrf_violation(
            &post,
            &headers(&[
                cookie,
                ("host", "h"),
                ("authorization", "Basic dXNlcjpwYXNz"),
                ("origin", "http://evil.com")
            ]),
            C,
            None,
            None
        ));
        // Trusted proxy header auth enforces Origin check even without session cookie
        assert!(is_csrf_violation(
            &post,
            &headers(&[
                ("remote-user", "alice"),
                ("host", "h"),
                ("origin", "http://evil.com")
            ]),
            C,
            Some("remote-user"),
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[
                ("remote-user", "alice"),
                ("host", "h"),
                ("origin", "https://h")
            ]),
            C,
            Some("remote-user"),
            None
        ));
        // safe methods, no cookie, API key and bearer exempt
        assert!(!is_csrf_violation(
            &Method::GET,
            &headers(&[cookie, ("host", "h"), ("origin", "http://evil.com")]),
            C,
            None,
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[("host", "h"), ("origin", "http://evil.com")]),
            C,
            None,
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[
                cookie,
                ("host", "h"),
                ("apikey", "k"),
                ("origin", "http://evil.com")
            ]),
            C,
            None,
            None
        ));
        assert!(!is_csrf_violation(
            &post,
            &headers(&[cookie, ("host", "h"), ("authorization", "Bearer x")]),
            C,
            None,
            None
        ));
    }
}
