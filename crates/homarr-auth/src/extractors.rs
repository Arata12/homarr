use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use homarr_common::{ip_matches_any, HomarrError};
use homarr_core::AppConfig;
use homarr_db::{repos, Database};
use homarr_definitions::GroupPermissionKey;

use crate::api_key::{authenticate_api_key, API_KEY_HEADER_NAME};
use crate::permissions::AuthSession;
use crate::session::{get_session_from_token, session_cookie_name};

pub trait AuthContextProvider: Send + Sync {
    fn db(&self) -> &Database;
    fn config(&self) -> &AppConfig;
}

pub struct OptionalAuth(pub Option<AuthSession>);
pub struct RequireAuth(pub AuthSession);
pub struct RequireAdmin(pub AuthSession);

fn extract_cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

async fn resolve_session<S: AuthContextProvider>(
    parts: &Parts,
    state: &S,
) -> Result<Option<AuthSession>, HomarrError> {
    // 1. Check ApiKey header (never fall back to session cookie when ApiKey is supplied)
    if parts.headers.contains_key(API_KEY_HEADER_NAME) {
        let Some(api_key_val) = parts
            .headers
            .get(API_KEY_HEADER_NAME)
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(None);
        };
        return authenticate_api_key(state.db(), api_key_val).await;
    }

    // 2. Check Authorization: Bearer <id.secret> (never fall back to cookie when Bearer is supplied)
    if let Some(auth_val) = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = auth_val
            .strip_prefix("Bearer ")
            .or_else(|| auth_val.strip_prefix("bearer "))
        {
            return authenticate_api_key(state.db(), token).await;
        }
    }

    // 3. Check session cookie (iterate all Cookie headers for HTTP/2 compatibility)
    let cookie_name = session_cookie_name(&state.config().auth_cookie_prefix);
    for cookie_hdr in parts
        .headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
    {
        if let Some(token) = extract_cookie_value(cookie_hdr, &cookie_name) {
            if let Some(sess) = get_session_from_token(state.db(), &token).await? {
                return Ok(Some(sess));
            }
        }
    }

    // 4. Check trusted proxy header if configured, only from an allowed proxy peer address
    if let Some(ref header_name) = state.config().auth_trusted_proxy_header {
        let peer_trusted = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|ConnectInfo(addr)| {
                ip_matches_any(addr.ip(), &state.config().auth_trusted_proxy_ips)
            });
        if let Some(username) = parts
            .headers
            .get(header_name.as_str())
            .and_then(|v| v.to_str().ok())
            .filter(|_| peer_trusted)
        {
            if let Some(user) = repos::users::find_by_name_ci(state.db(), username).await? {
                let permissions = repos::groups::get_user_permissions(state.db(), &user.id).await?;
                let group_ids = repos::groups::get_user_group_ids(state.db(), &user.id).await?;
                return Ok(Some(AuthSession::from_user(
                    user.id,
                    user.name,
                    user.email,
                    user.image,
                    user.color_scheme,
                    permissions,
                    group_ids,
                    false,
                    None,
                )));
            }
        }
    }

    Ok(None)
}

impl<S> FromRequestParts<S> for OptionalAuth
where
    S: AuthContextProvider,
{
    type Rejection = HomarrError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let sess = resolve_session(parts, state).await?;
        Ok(Self(sess))
    }
}

impl<S> FromRequestParts<S> for RequireAuth
where
    S: AuthContextProvider,
{
    type Rejection = HomarrError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let sess = resolve_session(parts, state)
            .await?
            .ok_or_else(|| HomarrError::Unauthorized("Authentication required".to_string()))?;
        Ok(Self(sess))
    }
}

impl<S> FromRequestParts<S> for RequireAdmin
where
    S: AuthContextProvider,
{
    type Rejection = HomarrError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let sess = resolve_session(parts, state)
            .await?
            .ok_or_else(|| HomarrError::Unauthorized("Authentication required".to_string()))?;
        sess.require_permission(GroupPermissionKey::Admin)?;
        Ok(Self(sess))
    }
}
