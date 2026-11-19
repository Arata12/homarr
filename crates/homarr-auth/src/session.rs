use chrono::Utc;
use homarr_common::{generate_secure_random_token, HomarrResult};
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
    let expires_ms = Utc::now().timestamp_millis() + ttl_seconds * 1000;
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

pub fn build_session_cookie(cookie_prefix: &str, token: String, max_age_secs: i64) -> String {
    format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}",
        session_cookie_name(cookie_prefix)
    )
}

pub fn build_clear_session_cookie(cookie_prefix: &str) -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        session_cookie_name(cookie_prefix)
    )
}
