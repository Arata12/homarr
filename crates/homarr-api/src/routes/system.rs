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
    repos::{assistant, boards, settings, users},
};
use homarr_definitions::settings::{
    default_server_setting_json, parse_branding_settings, AnalyticsSettings, AppearanceSettings,
    BoardServerSettings, BrandingSettings, CrawlingSettings, CultureSettings, SearchServerSettings,
    ThemeId, UserServerSettings, WhatsNewSettings, DEFAULT_SERVER_SETTINGS_KEYS,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};

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
    // Validate every key before writing any, so a bad section can't leave a partial save.
    let mut validated = Vec::with_capacity(patch.len());
    for (k, v) in patch {
        let value = validate_setting_patch(&state, &k, v).await?;
        validated.push((k, value));
    }
    for (k, v) in validated {
        settings::set_setting(&state.db, &k, &v).await?;
    }
    let updated = settings::get_all_server_settings(&state.db).await?;
    Ok(Json(updated))
}

fn bad_setting(key: &str, e: impl std::fmt::Display) -> HomarrError {
    HomarrError::BadRequest(format!("Invalid value for setting '{key}': {e}"))
}

fn setting_to_json<T: Serialize>(key: &str, value: &T) -> HomarrResult<Value> {
    serde_json::to_value(value).map_err(|e| HomarrError::Internal(format!("{key}: {e}")))
}

/// Defaults <- stored value <- patch, type-checked against the setting's struct.
fn merge_typed<T: DeserializeOwned>(
    key: &str,
    current: &Value,
    patch: Map<String, Value>,
) -> HomarrResult<T> {
    let mut merged = default_server_setting_json(key)
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if let Some(cur) = current.as_object() {
        merged.extend(cur.clone());
    }
    merged.extend(patch);
    serde_json::from_value(Value::Object(merged)).map_err(|e| bad_setting(key, e))
}

fn valid_branding_image_url(v: &str) -> bool {
    v.chars().count() <= 2048
        && ((v.starts_with('/') && !v.starts_with("//"))
            || url::Url::parse(v).is_ok_and(|u| matches!(u.scheme(), "http" | "https")))
}

/// Port of serverSettings.saveSettings: known keys only, value shapes checked per key.
async fn validate_setting_patch(state: &AppState, key: &str, value: Value) -> HomarrResult<Value> {
    if !DEFAULT_SERVER_SETTINGS_KEYS.contains(&key) {
        return Err(HomarrError::BadRequest(format!("Unknown setting '{key}'")));
    }
    let Value::Object(mut patch) = value else {
        return Err(bad_setting(key, "value must be an object"));
    };
    let current = settings::get_setting(&state.db, key).await?;
    match key {
        "board" => {
            // saveBoardSettingsAsync: home boards must exist and be public.
            let b: BoardServerSettings = merge_typed(key, &current, patch)?;
            for id in [&b.home_board_id, &b.mobile_home_board_id]
                .into_iter()
                .flatten()
            {
                let public = boards::get_by_id(&state.db, id)
                    .await?
                    .is_some_and(|r| r.is_public);
                if !public {
                    return Err(HomarrError::BadRequest(format!(
                        "Board settings home board IDs must reference public boards: {id}"
                    )));
                }
            }
            setting_to_json(key, &b)
        }
        "analytics" => {
            if let Some(k) = patch.keys().find(|k| k.as_str() != "enableGeneral") {
                return Err(bad_setting(key, format!("unknown field '{k}'")));
            }
            setting_to_json(
                key,
                &merge_typed::<AnalyticsSettings>(key, &current, patch)?,
            )
        }
        "branding" => {
            let current = parse_branding_settings(&current);
            let mut base = setting_to_json(key, &current)?
                .as_object()
                .cloned()
                .unwrap_or_default();
            let mut auth = base
                .get("authBranding")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for (legacy, field) in [
                ("showCustomAppNameOnLogin", "showAppName"),
                ("showCustomLogoOnLogin", "showLogo"),
                ("showCustomGreetingOnLogin", "showGreeting"),
            ] {
                if let Some(v) = patch.remove(legacy) {
                    auth.insert(field.to_string(), v);
                }
            }
            if let Some(ab) = patch.remove("authBranding") {
                let Value::Object(ab) = ab else {
                    return Err(bad_setting(key, "authBranding must be an object"));
                };
                auth.extend(ab);
            }
            base.extend(patch);
            base.insert("authBranding".to_string(), Value::Object(auth));
            let mut b: BrandingSettings =
                serde_json::from_value(Value::Object(base)).map_err(|e| bad_setting(key, e))?;
            b.app_name = b.app_name.trim().to_string();
            b.greeting = b.greeting.trim().to_string();
            if b.app_name.is_empty() || b.app_name.chars().count() > 80 {
                return Err(bad_setting(key, "appName must be 1-80 characters"));
            }
            if b.greeting.chars().count() > 160 {
                return Err(bad_setting(key, "greeting must be at most 160 characters"));
            }
            if !(0.0..=0.9).contains(&b.sign_in_background_overlay) {
                return Err(bad_setting(key, "signInBackgroundOverlay must be 0-0.9"));
            }
            for url in [
                &mut b.logo_image_url,
                &mut b.favicon_image_url,
                &mut b.sign_in_background_image_url,
            ]
            .into_iter()
            .flatten()
            {
                *url = url.trim().to_string();
                if !valid_branding_image_url(url) {
                    return Err(bad_setting(
                        key,
                        "image must be an HTTP(S) URL or an app-relative path",
                    ));
                }
            }
            // shortcut: customTheme is only checked to be an object, not against customThemeSchema.
            if b.custom_theme.as_ref().is_some_and(|t| !t.is_object()) {
                return Err(bad_setting(key, "customTheme must be an object"));
            }
            if b.theme_id == ThemeId::Custom && b.custom_theme.is_none() {
                return Err(HomarrError::BadRequest(
                    "Save a custom theme before selecting it".into(),
                ));
            }
            setting_to_json(key, &b)
        }
        "crawlingAndIndexing" => {
            setting_to_json(key, &merge_typed::<CrawlingSettings>(key, &current, patch)?)
        }
        "user" => setting_to_json(
            key,
            &merge_typed::<UserServerSettings>(key, &current, patch)?,
        ),
        "appearance" => setting_to_json(
            key,
            &merge_typed::<AppearanceSettings>(key, &current, patch)?,
        ),
        "culture" => setting_to_json(key, &merge_typed::<CultureSettings>(key, &current, patch)?),
        "search" => setting_to_json(
            key,
            &merge_typed::<SearchServerSettings>(key, &current, patch)?,
        ),
        "whatsNew" => setting_to_json(key, &merge_typed::<WhatsNewSettings>(key, &current, patch)?),
        _ => Err(HomarrError::BadRequest(format!("Unknown setting '{key}'"))),
    }
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
    // Read-only API keys and demo mode must not mutate onboarding state.
    match &session {
        Some(s) => s.require_writable(state.demo_read_only_active())?,
        None if state.demo_read_only_active() => {
            return Err(HomarrError::Forbidden("Demo mode is read-only".into()));
        }
        None => {}
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
    state.http.reload()?;
    Ok((StatusCode::CREATED, Json(json!({ "ok": true }))))
}

pub async fn delete_certificate(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    state.certs.remove(&name)?;
    state.http.reload()?;
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
    let summary = import_backup_json(&state.db, &bundle).await?;
    Ok(Json(json!({ "ok": true, "summary": summary })))
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
    let base_url = normalize_assistant_base_url(&body.base_url)?;
    let existing = assistant::get_config(&state.db).await?;
    // Stored credentials must never be sent to a different provider/host.
    let destination_changed = existing.provider != body.provider || existing.base_url != base_url;
    let encrypted_api_key = if let Some(raw) = body.api_key.filter(|s| !s.trim().is_empty()) {
        Some(state.crypto.encrypt_secret(raw.trim())?)
    } else if destination_changed {
        None
    } else {
        existing.encrypted_api_key
    };
    let encrypted_headers = if destination_changed {
        None
    } else {
        existing.encrypted_headers
    };
    assistant::save_config(
        &state.db,
        &AssistantConfigRecord {
            id: "default".to_string(),
            enabled: body.enabled,
            web_search_enabled: body.web_search_enabled,
            provider: body.provider,
            base_url,
            model_discovery_path: body.model_discovery_path,
            encrypted_api_key,
            encrypted_headers,
            model_id: body.model_id,
            updated_at: 0,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

fn normalize_assistant_base_url(value: &str) -> HomarrResult<String> {
    let url = url::Url::parse(value.trim())
        .map_err(|_| HomarrError::BadRequest("Enter a valid provider URL.".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(HomarrError::BadRequest(
            "Provider URL must use HTTP(S) without credentials, query or fragment.".into(),
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
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
