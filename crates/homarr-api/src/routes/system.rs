use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{OptionalAuth, RequireAdmin, RequireAuth};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    backup::{export_backup_json, import_backup_json, HomarrBackupBundle},
    models::AssistantConfigRecord,
    repos::{assistant, settings, users},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn get_settings(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<HashMap<String, Value>>> {
    let all = settings::get_all_server_settings(&state.db).await?;
    Ok(Json(all))
}

pub async fn patch_settings(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(patch): Json<HashMap<String, Value>>,
) -> HomarrResult<Json<HashMap<String, Value>>> {
    admin.require_writable(state.demo_read_only_active())?;
    for (k, v) in patch {
        settings::set_setting(&state.db, &k, &v).await?;
    }
    let updated = settings::get_all_server_settings(&state.db).await?;
    Ok(Json(updated))
}

pub async fn get_onboarding(State(state): State<AppState>) -> HomarrResult<Json<Value>> {
    let record = settings::get_onboarding(&state.db).await?;
    let user_count = homarr_db::repos::users::count(&state.db).await?;
    Ok(Json(json!({
        "onboarding": record,
        "hasUsers": user_count > 0,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingBody {
    pub step: String,
    pub previous_step: Option<String>,
}

pub async fn update_onboarding(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Json(body): Json<OnboardingBody>,
) -> HomarrResult<Json<Value>> {
    if state.demo_read_only_active() {
        return Err(HomarrError::Forbidden("Demo mode is read-only".into()));
    }
    // Anonymous onboarding is only allowed before the first user exists.
    if users::count(&state.db).await? > 0 {
        let session =
            session.ok_or_else(|| HomarrError::Unauthorized("Authentication required".into()))?;
        session.require_permission(homarr_definitions::GroupPermissionKey::Admin)?;
    }
    let rec =
        settings::set_onboarding(&state.db, &body.step, body.previous_step.as_deref()).await?;
    Ok(Json(json!({ "onboarding": rec })))
}

pub async fn list_cron_jobs(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> Json<Value> {
    let jobs = state.scheduler.list_jobs(false).await;
    Json(json!({ "items": jobs }))
}

pub async fn trigger_cron_job(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let status = state.scheduler.trigger_job(&name).await?;
    Ok(Json(serde_json::to_value(status).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCronJobBody {
    pub cron_expression: Option<String>,
    pub is_enabled: Option<bool>,
}

pub async fn update_cron_job(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateCronJobBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let status = state
        .scheduler
        .update_job(&name, body.cron_expression.as_deref(), body.is_enabled)
        .await?;
    Ok(Json(serde_json::to_value(status).unwrap_or_default()))
}

pub async fn list_certificates(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let files = state.certs.list()?;
    let hostnames = settings::list_trusted_hostnames(&state.db).await?;
    Ok(Json(json!({
        "files": files,
        "hostnames": hostnames
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadCertificateBody {
    pub file_name: String,
    pub content: String,
}

pub async fn upload_certificate(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<UploadCertificateBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    state.certs.save(&body.file_name, &body.content)?;
    Ok((StatusCode::CREATED, Json(json!({ "ok": true }))))
}

pub async fn delete_certificate(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    state.certs.remove(&name)?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct RecentLogsQuery {
    pub limit: Option<usize>,
}

pub async fn recent_logs(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    Query(q): Query<RecentLogsQuery>,
) -> Json<Value> {
    let entries = state.logs.recent(q.limit.unwrap_or(200).clamp(1, 1000));
    Json(json!({ "items": entries }))
}

pub async fn export_backup(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<HomarrBackupBundle>> {
    let bundle = export_backup_json(&state.db).await?;
    Ok(Json(bundle))
}

pub async fn import_backup(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(bundle): Json<HomarrBackupBundle>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    import_backup_json(&state.db, &bundle).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn get_assistant_config(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let cfg = assistant::get_config(&state.db).await?;
    Ok(Json(json!({
        "enabled": cfg.enabled,
        "webSearchEnabled": cfg.web_search_enabled,
        "provider": cfg.provider,
        "baseUrl": cfg.base_url,
        "modelDiscoveryPath": cfg.model_discovery_path,
        "hasApiKey": cfg.encrypted_api_key.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        "modelId": cfg.model_id,
        "updatedAt": cfg.updated_at,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAssistantConfigBody {
    pub enabled: bool,
    #[serde(default)]
    pub web_search_enabled: bool,
    pub provider: String,
    pub base_url: String,
    pub model_discovery_path: Option<String>,
    pub api_key: Option<String>,
    pub model_id: Option<String>,
}

pub async fn save_assistant_config(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<SaveAssistantConfigBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let encrypted_api_key = if let Some(raw) = body.api_key.filter(|s| !s.trim().is_empty()) {
        Some(state.crypto.encrypt_secret(raw.trim())?)
    } else {
        None
    };
    assistant::save_config(
        &state.db,
        &AssistantConfigRecord {
            id: "default".to_string(),
            enabled: body.enabled,
            web_search_enabled: body.web_search_enabled,
            provider: body.provider,
            base_url: body.base_url,
            model_discovery_path: body.model_discovery_path,
            encrypted_api_key,
            encrypted_headers: None,
            model_id: body.model_id,
            updated_at: 0,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn list_assistant_threads(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let items = assistant::list_threads(&state.db, &session.user_id).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateThreadBody {
    pub title: Option<String>,
    pub model_id: Option<String>,
}

pub async fn create_assistant_thread(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<CreateThreadBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    session.require_writable(state.demo_read_only_active())?;
    let thread = assistant::create_thread(
        &state.db,
        &session.user_id,
        body.title.as_deref(),
        body.model_id.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "thread": thread }))))
}

pub async fn delete_assistant_thread(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    let deleted = assistant::delete_thread(&state.db, &id, &session.user_id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Assistant thread not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

pub async fn list_assistant_messages(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    let threads = assistant::list_threads(&state.db, &session.user_id).await?;
    if !threads.iter().any(|t| t.id == id) {
        return Err(HomarrError::Forbidden("Thread access denied".into()));
    }
    let msgs = assistant::list_messages(&state.db, &id).await?;
    Ok(Json(json!({ "items": msgs })))
}

#[derive(Deserialize)]
pub struct SendAssistantMessageBody {
    pub prompt: String,
}

pub async fn send_assistant_message(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendAssistantMessageBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    session.require_writable(state.demo_read_only_active())?;
    let threads = assistant::list_threads(&state.db, &session.user_id).await?;
    if !threads.iter().any(|t| t.id == id) {
        return Err(HomarrError::Forbidden("Thread access denied".into()));
    }
    let (user_msg, reply) = state
        .services
        .assistant
        .send_user_message(&state.db, &id, &body.prompt)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "userMessage": user_msg, "assistantMessage": reply })),
    ))
}
