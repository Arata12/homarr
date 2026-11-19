use axum::{
    extract::{Path, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use homarr_auth::{
    build_clear_session_cookie, build_session_cookie, cookie_secure, create_api_key,
    create_session, delete_session, hash_password, session_cookie_name, verify_password,
    OptionalAuth, RequireAdmin,
};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::repos::{api_keys, boards, groups, invites, settings, users};
use homarr_db::seed;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

static REGISTER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginBody {
    pub name: String,
    pub password: String,
}

pub async fn login(
    State(state): State<AppState>,
    req_headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> HomarrResult<impl IntoResponse> {
    let user = users::find_by_name_ci(&state.db, body.name.trim())
        .await?
        .ok_or_else(|| HomarrError::Unauthorized("Invalid username or password".into()))?;
    let hash = user
        .password
        .as_deref()
        .ok_or_else(|| HomarrError::Unauthorized("User has no password configured".into()))?;
    if !verify_password(&body.password, hash).await? {
        return Err(HomarrError::Unauthorized(
            "Invalid username or password".into(),
        ));
    }

    let token = create_session(&state.db, &user.id, state.config.auth_session_expiry_secs).await?;
    let cookie = build_session_cookie(
        &state.config.auth_cookie_prefix,
        token,
        state.config.auth_session_expiry_secs,
        cookie_secure(&state.config, &req_headers),
    );

    let mut headers = HeaderMap::new();
    if let Ok(hv) = HeaderValue::from_str(&cookie) {
        headers.insert(SET_COOKIE, hv);
    }

    Ok((
        StatusCode::OK,
        headers,
        Json(json!({
            "user": {
                "id": user.id,
                "name": user.name,
                "email": user.email,
                "colorScheme": user.color_scheme
            }
        })),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    req_headers: HeaderMap,
) -> HomarrResult<impl IntoResponse> {
    let cookie_name = session_cookie_name(&state.config.auth_cookie_prefix);
    for cookie_hdr in req_headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
    {
        for part in cookie_hdr.split(';') {
            if let Some((k, v)) = part.trim().split_once('=') {
                if k.trim() == cookie_name {
                    let _ = delete_session(&state.db, v.trim()).await;
                }
            }
        }
    }
    let cleared = build_clear_session_cookie(
        &state.config.auth_cookie_prefix,
        cookie_secure(&state.config, &req_headers),
    );
    let mut headers = HeaderMap::new();
    if let Ok(hv) = HeaderValue::from_str(&cleared) {
        headers.insert(SET_COOKIE, hv);
    }
    Ok((StatusCode::OK, headers, Json(json!({ "ok": true }))))
}

pub async fn session_info(OptionalAuth(session): OptionalAuth) -> Json<Value> {
    match session {
        Some(s) => Json(json!({
            "authenticated": true,
            "session": s
        })),
        None => Json(json!({
            "authenticated": false,
            "session": Value::Null
        })),
    }
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub name: String,
    pub password: String,
    pub email: Option<String>,
    pub invite_token: Option<String>,
    #[serde(default)]
    pub board_name: Option<String>,
    #[serde(default)]
    pub color_scheme: Option<String>,
    #[serde(default = "default_true")]
    pub create_starter_board: bool,
}

pub async fn register(
    State(state): State<AppState>,
    req_headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> HomarrResult<impl IntoResponse> {
    let username = body.name.trim().to_string();
    if username.len() < 2 || body.password.chars().count() < 8 {
        return Err(HomarrError::BadRequest(
            "Username must be >= 2 chars and password >= 8 chars".into(),
        ));
    }

    // Serializes the first-admin check and invite consumption so concurrent requests cannot
    // both become admin or reuse one invite.
    // shortcut: process-local lock, needs a DB-level guard if Homarr ever runs multiple replicas.
    let _register_guard = REGISTER_LOCK.lock().await;
    let existing_count = users::count(&state.db).await?;
    let mut invite_id_to_delete = None;

    if existing_count > 0 {
        let token = body
            .invite_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                HomarrError::Forbidden("Registration requires a valid invite token".into())
            })?;
        let invite = invites::find_by_token(&state.db, token)
            .await?
            .ok_or_else(|| HomarrError::Forbidden("Invalid invite token".into()))?;
        if invite.expiration_date <= Utc::now().timestamp() {
            return Err(HomarrError::Forbidden("Invite token has expired".into()));
        }
        invite_id_to_delete = Some(invite.id);
    }

    if users::find_by_name_ci(&state.db, &username)
        .await?
        .is_some()
    {
        return Err(HomarrError::Conflict("Username already exists".into()));
    }

    let password_hash = hash_password(&body.password).await?;
    let user = users::create(
        &state.db,
        users::CreateUserInput {
            id: None,
            name: username,
            email: body.email,
            password_hash: Some(password_hash),
            provider: "credentials".to_string(),
            image: None,
        },
    )
    .await?;

    if let Some(everyone) = groups::get_by_name(&state.db, "everyone").await? {
        let _ = groups::add_member(&state.db, &everyone.id, &user.id).await;
    }

    if existing_count == 0 {
        let _ = seed::seed_defaults(&state.db).await;
        let admin_group = match groups::get_by_name(&state.db, "administrators").await? {
            Some(g) => g,
            None => {
                let g = groups::create(&state.db, "administrators", Some(&user.id), 0).await?;
                groups::set_permissions(&state.db, &g.id, &["admin".to_string()]).await?;
                g
            }
        };
        let _ = groups::add_member(&state.db, &admin_group.id, &user.id).await;

        if let Some(scheme) = body
            .color_scheme
            .as_deref()
            .map(str::trim)
            .filter(|s| matches!(*s, "light" | "dark"))
        {
            let _ = users::update_preferences(
                &state.db,
                &user.id,
                users::UpdateUserPreferencesInput {
                    color_scheme: Some(scheme.to_string()),
                    ..Default::default()
                },
            )
            .await;
            let _ = settings::set_setting(
                &state.db,
                "appearance",
                &json!({ "defaultColorScheme": scheme }),
            )
            .await;
        }

        if body.create_starter_board && boards::list_all(&state.db).await?.is_empty() {
            let _ = seed::seed_starter_board(
                &state.db,
                &user.id,
                body.board_name.as_deref().unwrap_or("dashboard"),
                10,
            )
            .await?;
        }
    }

    if let Some(iid) = invite_id_to_delete {
        let _ = invites::delete(&state.db, &iid).await;
    }

    let token = create_session(&state.db, &user.id, state.config.auth_session_expiry_secs).await?;
    let cookie = build_session_cookie(
        &state.config.auth_cookie_prefix,
        token,
        state.config.auth_session_expiry_secs,
        cookie_secure(&state.config, &req_headers),
    );
    let mut headers = HeaderMap::new();
    if let Ok(hv) = HeaderValue::from_str(&cookie) {
        headers.insert(SET_COOKIE, hv);
    }

    Ok((StatusCode::CREATED, headers, Json(json!({ "user": user }))))
}

pub async fn list_api_keys(
    RequireAdmin(session): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let keys = api_keys::list_by_user(&state.db, &session.user_id).await?;
    Ok(Json(json!({ "items": keys })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyBody {
    #[serde(default)]
    pub read_only: bool,
}

pub async fn create_user_api_key(
    RequireAdmin(session): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CreateApiKeyBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    session.require_writable(state.demo_read_only_active())?;
    let (plaintext, record) =
        create_api_key(&state.db, &session.user_id, body.read_only, None).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "apiKey": plaintext,
            "record": record
        })),
    ))
}

pub async fn delete_user_api_key(
    RequireAdmin(session): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    let deleted = api_keys::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("API key not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}
