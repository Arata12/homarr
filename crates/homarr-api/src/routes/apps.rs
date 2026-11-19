use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{can_view_board, AuthSession, OptionalAuth, RequireAuth};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    models::AppRecord,
    repos::{
        apps::{self, AppInput},
        boards, settings,
    },
};
use homarr_definitions::GroupPermissionKey;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn list_apps(State(state): State<AppState>) -> HomarrResult<Json<Value>> {
    let items = apps::list_all(&state.db).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBody {
    pub name: String,
    pub description: Option<String>,
    pub icon_url: String,
    pub href: Option<String>,
    pub ping_url: Option<String>,
}

pub async fn create_app(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<AppBody>,
) -> HomarrResult<(StatusCode, Json<AppRecord>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::AppCreate)?;
    let app = apps::create(
        &state.db,
        AppInput {
            id: None,
            name: body.name.trim().to_string(),
            description: body.description,
            icon_url: body.icon_url,
            href: body.href,
            ping_url: body.ping_url,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(app)))
}

pub async fn get_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<AppRecord>> {
    let app = apps::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("App not found".into()))?;
    Ok(Json(app))
}

pub async fn update_app(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AppBody>,
) -> HomarrResult<Json<AppRecord>> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::AppModifyAll)?;
    let app = apps::update(
        &state.db,
        &id,
        AppInput {
            id: None,
            name: body.name.trim().to_string(),
            description: body.description,
            icon_url: body.icon_url,
            href: body.href,
            ping_url: body.ping_url,
        },
    )
    .await?;
    Ok(Json(app))
}

pub async fn delete_app(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::AppFullAll)?;
    let deleted = apps::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("App not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingBody {
    pub app_id: String,
}

/// Pings an app's stored URL. Only apps placed on a board the caller can view
/// (or any app for admins) may be pinged, so the endpoint can't probe arbitrary hosts.
pub async fn ping_url(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Json(body): Json<PingBody>,
) -> HomarrResult<Json<Value>> {
    let board_cfg = settings::get_setting(&state.db, "board").await?;
    if board_cfg
        .get("forceDisableStatus")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(HomarrError::Forbidden(
            "Ping functionality is disabled by server settings".into(),
        ));
    }

    // Same 404 for "missing" and "not visible" to avoid app-id enumeration.
    let not_found = || HomarrError::NotFound("App not found".into());
    let app = apps::get_by_id(&state.db, &body.app_id)
        .await?
        .ok_or_else(not_found)?;
    let is_admin = session.as_ref().is_some_and(|s| s.is_admin());
    if !is_admin && !app_on_viewable_board(&state, session.as_ref(), &app.id).await? {
        return Err(not_found());
    }

    let url = app
        .ping_url
        .as_deref()
        .or(app.href.as_deref())
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| HomarrError::Conflict("No URL to ping configured for this app".into()))?;
    let res = state.services.ping.ping_url(url).await;
    Ok(Json(serde_json::to_value(res).unwrap_or_default()))
}

async fn app_on_viewable_board(
    state: &AppState,
    session: Option<&AuthSession>,
    app_id: &str,
) -> HomarrResult<bool> {
    for board in boards::list_all(&state.db).await? {
        if !can_view_board(&state.db, session, &board).await? {
            continue;
        }
        let Some(full) = boards::get_full_board_by_id(&state.db, &board.id).await? else {
            continue;
        };
        if full.items.iter().any(|item| {
            item.kind == "app" && item.options.get("appId").and_then(|v| v.as_str()) == Some(app_id)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}
