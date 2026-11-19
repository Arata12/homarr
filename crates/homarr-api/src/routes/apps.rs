use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::RequireAuth;
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    models::AppRecord,
    repos::apps::{self, AppInput},
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
    pub url: String,
}

pub async fn ping_url(State(state): State<AppState>, Json(body): Json<PingBody>) -> Json<Value> {
    let res = state.services.ping.ping_url(&body.url).await;
    Json(serde_json::to_value(res).unwrap_or_default())
}
