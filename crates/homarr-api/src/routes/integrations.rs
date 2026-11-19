use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{check_integration_permission, RequireAuth};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    models::{GroupPermissionEntry, IntegrationRecord, UserPermissionEntry},
    repos::integrations::{self, IntegrationInput},
};
use homarr_definitions::{
    integration::all_integration_defs, GroupPermissionKey, IntegrationKind, IntegrationPermission,
};
use homarr_integrations::IntegrationContext;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn list_kinds() -> Json<Value> {
    let kinds: Vec<Value> = all_integration_defs()
        .iter()
        .map(|d| {
            let secret_sets: Vec<Vec<&str>> = d
                .secret_kinds
                .iter()
                .map(|set| set.iter().map(|sk| sk.as_str()).collect())
                .collect();
            json!({
                "kind": d.kind_str,
                "name": d.name,
                "iconUrl": d.icon_url,
                "categories": d.categories.iter().map(|c| format!("{c:?}").to_lowercase()).collect::<Vec<_>>(),
                "secretKinds": secret_sets,
            })
        })
        .collect();
    Json(json!({ "items": kinds }))
}

pub async fn list_integrations(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let all = integrations::list_all(&state.db).await?;
    let mut visible = Vec::new();
    for item in all {
        if check_integration_permission(&state.db, &session, &item.id, IntegrationPermission::Use)
            .await
            .is_ok()
        {
            visible.push(item);
        }
    }
    Ok(Json(json!({ "items": visible })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPairBody {
    pub kind: String,
    pub value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIntegrationBody {
    pub name: String,
    pub url: String,
    pub external_url: Option<String>,
    pub kind: String,
    pub app_id: Option<String>,
    #[serde(default)]
    pub secrets: Vec<SecretPairBody>,
}

pub async fn create_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<CreateIntegrationBody>,
) -> HomarrResult<(StatusCode, Json<IntegrationRecord>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::IntegrationCreate)?;

    let kind = IntegrationKind::from_str_opt(body.kind.trim()).ok_or_else(|| {
        HomarrError::BadRequest(format!("Unknown integration kind '{}'", body.kind))
    })?;
    if kind == IntegrationKind::Mock && !state.config.unsafe_enable_mock_integration {
        return Err(HomarrError::Forbidden(
            "Mock integration is disabled (set UNSAFE_ENABLE_MOCK_INTEGRATION=true)".into(),
        ));
    }

    let mut enc_secrets = Vec::new();
    for s in &body.secrets {
        let enc = state.crypto.encrypt_secret(&s.value)?;
        enc_secrets.push((s.kind.clone(), enc));
    }

    let created = integrations::create(
        &state.db,
        IntegrationInput {
            id: None,
            name: body.name.trim().to_string(),
            url: body.url.trim().to_string(),
            external_url: body.external_url,
            kind: kind.as_str().to_string(),
            app_id: body.app_id,
        },
        &enc_secrets,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn get_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<IntegrationRecord>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Use).await?;
    let rec = integrations::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Integration not found".into()))?;
    Ok(Json(rec))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIntegrationBody {
    pub name: String,
    pub url: String,
    pub external_url: Option<String>,
    pub app_id: Option<String>,
    pub secrets: Option<Vec<SecretPairBody>>,
}

pub async fn update_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateIntegrationBody>,
) -> HomarrResult<Json<IntegrationRecord>> {
    session.require_writable(state.demo_read_only_active())?;
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Full).await?;

    let enc = if let Some(ref pairs) = body.secrets {
        let mut out = Vec::with_capacity(pairs.len());
        for s in pairs {
            out.push((s.kind.clone(), state.crypto.encrypt_secret(&s.value)?));
        }
        Some(out)
    } else {
        None
    };

    let updated = integrations::update(
        &state.db,
        &id,
        body.name.trim(),
        body.url.trim(),
        body.external_url.as_deref(),
        body.app_id.as_deref(),
        enc.as_deref(),
    )
    .await?;
    Ok(Json(updated))
}

pub async fn delete_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Full).await?;
    let deleted = integrations::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Integration not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

async fn load_ctx(state: &AppState, id: &str) -> HomarrResult<IntegrationContext> {
    let rec = integrations::get_by_id(&state.db, id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Integration not found".into()))?;
    let kind = IntegrationKind::from_str_opt(&rec.kind)
        .ok_or_else(|| HomarrError::BadRequest("Invalid integration kind".into()))?;
    let raw_secrets = integrations::get_secrets(&state.db, id).await?;
    let mut secrets = HashMap::new();
    for s in raw_secrets {
        if let Ok(plain) = state.crypto.decrypt_secret(&s.value) {
            secrets.insert(s.kind, plain);
        }
    }
    Ok(IntegrationContext {
        id: rec.id,
        name: rec.name,
        url: rec.url,
        external_url: rec.external_url,
        kind,
        secrets,
    })
}

pub async fn test_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Interact).await?;
    let ctx = load_ctx(&state, &id).await?;
    let res = state.integrations.test_connection(&ctx).await;
    Ok(Json(serde_json::to_value(res).unwrap_or_default()))
}

pub async fn get_integration_data(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Use).await?;
    let ctx = load_ctx(&state, &id).await?;
    let stats = state.integrations.get_stats(&ctx).await?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

pub async fn get_integration_permissions(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Full).await?;
    let (users, groups) = integrations::get_permissions(&state.db, &id).await?;
    Ok(Json(json!({ "users": users, "groups": groups })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationPermissionsBody {
    #[serde(default)]
    pub users: Vec<UserPermissionEntry>,
    #[serde(default)]
    pub groups: Vec<GroupPermissionEntry>,
}

pub async fn set_integration_permissions(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<IntegrationPermissionsBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Full).await?;
    integrations::set_permissions(&state.db, &id, &body.users, &body.groups).await?;
    Ok(Json(json!({ "ok": true })))
}
