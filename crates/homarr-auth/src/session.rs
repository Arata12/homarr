use axum::http::HeaderMap;
use chrono::Utc;
use homarr_common::{generate_secure_random_token, HomarrResult};
use homarr_core::AppConfig;
use homarr_db::{repos, Database};

use crate::permissions::AuthSession;

pub fn session_cookie_name(cookie_prefix: &str) -> String {
    format!("{cookie_prefix}.session-token")
}

pub async fn create_session(
    db: &Database,
    user_id: &str,
    ttl_seconds: i64,
) -> HomarrResult<String> {
    let token = generate_secure_random_token(48);
    let clamped_ttl = ttl_seconds.clamp(1, 365 * 10 * 86_400);
    let expires_ms = Utc::now()
        .timestamp_millis()
        .saturating_add(clamped_ttl.saturating_mul(1000));
    repos::sessions::create(db, &token, user_id, expires_ms).await?;
    Ok(token)
}

pub async fn get_session_from_token(
    db: &Database,
    token: &str,
) -> HomarrResult<Option<AuthSession>> {
    if token.trim().is_empty() {
        return Ok(None);
    }
    let Some(sess) = repos::sessions::find_with_user(db, token).await? else {
        return Ok(None);
    };
    if sess.expires_ms <= Utc::now().timestamp_millis() {
        let _ = repos::sessions::delete_by_token(db, token).await;
        return Ok(None);
    }
    let permissions = repos::groups::get_user_permissions(db, &sess.user_id).await?;
    let group_ids = repos::groups::get_user_group_ids(db, &sess.user_id).await?;
    Ok(Some(AuthSession::from_user(
        sess.user_id,
        sess.user_name,
        sess.user_email,
        sess.user_image,
        sess.user_color_scheme,
        permissions,
        group_ids,
        false,
        None,
    )))
}

pub async fn delete_session(db: &Database, token: &str) -> HomarrResult<()> {
    repos::sessions::delete_by_token(db, token).await
}

/// Decides whether the session cookie should carry `Secure`: explicit `AUTH_COOKIE_SECURE`
/// override first, then `X-Forwarded-Proto`, then an `https://` `BASE_URL`. Plain HTTP stays
/// usable by default.
pub fn cookie_secure(config: &AppConfig, headers: &HeaderMap) -> bool {
    if let Some(explicit) = config.auth_cookie_secure {
        return explicit;
    }
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
    {
        return proto
            .split(',')
            .next()
            .is_some_and(|p| p.trim().eq_ignore_ascii_case("https"));
    }
    config
        .base_url
        .as_deref()
        .is_some_and(|u| u.to_ascii_lowercase().starts_with("https://"))
}

pub fn build_session_cookie(
    cookie_prefix: &str,
    token: String,
    max_age_secs: i64,
    secure: bool,
) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}",
        session_cookie_name(cookie_prefix)
    )
}

pub fn build_clear_session_cookie(cookie_prefix: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        session_cookie_name(cookie_prefix)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_flag_decision() {
        let mut cfg = AppConfig::for_test();
        let mut h = HeaderMap::new();
        assert!(!cookie_secure(&cfg, &h));
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(cookie_secure(&cfg, &h));
        h.insert("x-forwarded-proto", "http".parse().unwrap());
        cfg.base_url = Some("https://homarr.example".into());
        assert!(!cookie_secure(&cfg, &h));
        assert!(cookie_secure(&cfg, &HeaderMap::new()));
        cfg.auth_cookie_secure = Some(false);
        assert!(!cookie_secure(&cfg, &HeaderMap::new()));
        assert!(build_session_cookie("homarr", "t".into(), 5, true).ends_with("; Secure"));
        assert!(!build_clear_session_cookie("homarr", false).contains("Secure"));
    }
}
