//! Frigate: the stats API needs a session. Port of `providers/frigate.ts` and `frigate-authentication-error.ts`:
//! log in over HTTPS with a bodyless cookie exchange, verify the profile, then call `/api/stats` with the token.

use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use reqwest::header::{HeaderMap, SET_COOKIE};
use reqwest::Method;
use serde_json::{json, Value};

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Count, Seconds, Text},
    StatsValue, StatsValues,
};
use crate::groups::stats::client::{parsed, Body, Headers, StatsClient};
use crate::groups::stats::json::{at, len, nonneg, object, text};

pub struct Frigate;

const METRICS: &[MetricDef] = &[
    m("cameras", "Cameras", Count),
    m("uptime", "Uptime", Seconds),
    m("version", "Version", Text),
];

const COOKIE_PREFIX: &str = "frigate_token=";
const MAX_TOKEN_LENGTH: usize = 4096;
const MAX_HEADER_BYTES: usize = 16_384;

/// Why Frigate authentication failed (TS `FrigateAuthenticationFailureReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Cookie,
    ProfileMismatch,
    Login,
    Profile,
    Stats,
}

impl Failure {
    fn error(self) -> HomarrError {
        let reason = match self {
            Self::Cookie => "cookie",
            Self::ProfileMismatch => "profileMismatch",
            Self::Login => "login",
            Self::Profile => "profile",
            Self::Stats => "stats",
        };
        HomarrError::Upstream(format!("Frigate authentication failed: {reason}"))
    }
}

#[async_trait]
impl StatsProvider for Frigate {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let headers = authenticate(client).await?;
        let response = client
            .get("/api/stats", &headers)
            .await
            .map_err(|_| Failure::Stats.error())?;
        parsed(client.ctx, "Frigate statistics", parse(&response))
    }
}

async fn authenticate(client: &StatsClient<'_>) -> HomarrResult<Headers> {
    if !client
        .ctx
        .base_url()
        .to_ascii_lowercase()
        .starts_with("https://")
    {
        return Err(HomarrError::BadRequest(
            "Frigate authentication requires an HTTPS integration URL to protect credentials"
                .to_string(),
        ));
    }
    let (Some(username), Some(password)) = (client.secret("username"), client.secret("password"))
    else {
        return Err(HomarrError::BadRequest(
            "Frigate username and password are required".to_string(),
        ));
    };
    let response_headers = client
        .send_for_headers(
            Method::POST,
            "/api/login",
            &Vec::new(),
            Body::Json(json!({ "user": username, "password": password })),
        )
        .await
        // The transport error may mention the login URL; keep only the failure class.
        .map_err(|_| Failure::Login.error())?;
    if header_bytes(&response_headers) > MAX_HEADER_BYTES {
        return Err(Failure::Login.error());
    }
    let cookies: Vec<&str> = response_headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    let token = session_token(&cookies).ok_or_else(|| Failure::Cookie.error())?;

    let headers: Headers = vec![("Authorization", format!("Bearer {token}"))];
    let profile = client
        .get("/api/profile", &headers)
        .await
        .map_err(|_| Failure::Profile.error())?;
    if !profile_matches(&profile, username) {
        return Err(Failure::ProfileMismatch.error());
    }
    Ok(headers)
}

fn header_bytes(headers: &HeaderMap) -> usize {
    headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
        .sum()
}

fn is_base64url_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Exactly one `frigate_token` cookie holding a JWT-shaped value (three base64url segments, bounded length).
fn session_token(set_cookies: &[&str]) -> Option<String> {
    let mut tokens = set_cookies
        .iter()
        .filter(|cookie| cookie.starts_with(COOKIE_PREFIX));
    let cookie = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    let token = cookie.split(';').next()?.strip_prefix(COOKIE_PREFIX)?;
    if token.is_empty() || token.len() > MAX_TOKEN_LENGTH {
        return None;
    }
    let segments: Vec<&str> = token.split('.').collect();
    (segments.len() == 3 && segments.iter().all(|s| is_base64url_segment(s)))
        .then(|| token.to_string())
}

/// The profile must belong to the configured user, be an admin and list its allowed cameras.
fn profile_matches(profile: &Value, username: &str) -> bool {
    profile.get("username").and_then(Value::as_str) == Some(username)
        && profile.get("role").and_then(Value::as_str) == Some("admin")
        && profile
            .get("allowed_cameras")
            .and_then(Value::as_array)
            .is_some_and(|cameras| cameras.iter().all(Value::is_string))
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let cameras = match response.get("cameras") {
        None | Some(Value::Null) => 0,
        Some(_) => object(response, &["cameras"])?.len(),
    };
    at(response, &["service"])?;
    Ok(values([
        ("cameras", len(cameras)),
        (
            "uptime",
            StatsValue::Number(nonneg(response, &["service", "uptime"])?),
        ),
        (
            "version",
            StatsValue::Text(text(response, &["service", "version"])?.to_string()),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhZG1pbiJ9.test_signature";

    #[test]
    fn accepts_a_single_jwt_cookie() {
        let cookie = format!("frigate_token={TOKEN}; HttpOnly; Path=/; SameSite=Lax");
        assert_eq!(session_token(&[&cookie]), Some(TOKEN.to_string()));
        assert_eq!(
            session_token(&["other=1", &cookie]),
            Some(TOKEN.to_string())
        );
    }

    #[test]
    fn rejects_missing_duplicate_malformed_and_oversized_cookies() {
        let cookie = format!("frigate_token={TOKEN}; Path=/");
        assert_eq!(session_token(&["other_cookie=value"]), None);
        assert_eq!(session_token(&[&cookie, &cookie]), None);
        assert_eq!(session_token(&["frigate_token=not-a-jwt"]), None);
        assert_eq!(session_token(&["frigate_token=a..c"]), None);
        let oversized = format!("frigate_token={}.b.c", "x".repeat(4097));
        assert_eq!(session_token(&[&oversized]), None);
    }

    #[test]
    fn profile_must_match_user_role_and_cameras() {
        let ok = json!({ "username": "operator", "role": "admin", "allowed_cameras": [] });
        assert!(profile_matches(&ok, "operator"));
        assert!(!profile_matches(&ok, "someone"));
        assert!(!profile_matches(
            &json!({ "username": "operator", "role": "viewer", "allowed_cameras": [] }),
            "operator"
        ));
        assert!(!profile_matches(
            &json!({ "username": "operator", "role": "admin" }),
            "operator"
        ));
        assert!(!profile_matches(
            &json!({ "username": "operator", "role": "admin", "allowed_cameras": [1] }),
            "operator"
        ));
    }

    #[test]
    fn maps_stats() {
        let out = parse(&json!({ "cameras": { "front": {} }, "service": { "uptime": 42, "version": "0.18.0" } })).unwrap();
        assert_eq!(out["cameras"], StatsValue::Number(1.0));
        assert_eq!(out["uptime"], StatsValue::Number(42.0));
        assert_eq!(out["version"], StatsValue::Text("0.18.0".into()));
        let out = parse(&json!({ "service": { "uptime": 1, "version": "x" } })).unwrap();
        assert_eq!(out["cameras"], StatsValue::Number(0.0));
        assert!(parse(&json!({ "cameras": {} })).is_err());
    }
}
