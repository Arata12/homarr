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
use homarr_integrations::{
    capabilities::{
        downloads::{DownloadClientItem, DownloadClientSelection},
        smart_home::SmartHomeAction,
    },
    IntegrationContext,
};
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

pub async fn get_integration_stats(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Use).await?;
    let ctx = load_ctx(&state, &id).await?;
    let metrics = state.integrations.stats_metrics(ctx.kind);
    if !metrics.is_empty() {
        let values = state.integrations.stats_values(&ctx).await?;
        let legacy = state.integrations.format_stats(&ctx, &metrics, &values);
        return Ok(Json(json!({
            "metrics": metrics,
            "values": values,
            "legacy": legacy,
        })));
    }
    let legacy = state.integrations.get_stats(&ctx).await?;
    Ok(Json(json!({
        "metrics": [],
        "values": {},
        "legacy": legacy,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationQueryBody {
    pub capability: String,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub input: Option<Value>,
}

fn is_mutating_integration_operation(operation: &str) -> bool {
    matches!(
        operation,
        "deleteNotification"
            | "updateNote"
            | "updateAlert"
            | "deleteAlert"
            | "pauseSystem"
            | "resumeSystem"
            | "deleteSystem"
    )
}

pub async fn query_integration(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<IntegrationQueryBody>,
) -> HomarrResult<Json<Value>> {
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Use).await?;
    let ctx = load_ctx(&state, &id).await?;
    let input = body.input.unwrap_or(Value::Null);
    let data = match body.capability.trim() {
        "firewall" => match body.operation.as_deref() {
            Some("cpu") => to_json_val(state.integrations.firewall_cpu(&ctx).await?),
            Some("memory") => to_json_val(state.integrations.firewall_memory(&ctx).await?),
            Some("interfaces") => to_json_val(state.integrations.firewall_interfaces(&ctx).await?),
            Some("version") => to_json_val(state.integrations.firewall_version(&ctx).await?),
            None | Some("all") => {
                let (cpu, memory, interfaces, version) = tokio::try_join!(
                    state.integrations.firewall_cpu(&ctx),
                    state.integrations.firewall_memory(&ctx),
                    state.integrations.firewall_interfaces(&ctx),
                    state.integrations.firewall_version(&ctx),
                )?;
                json!({
                    "cpu": cpu,
                    "memory": memory,
                    "interfaces": interfaces,
                    "version": version,
                })
            }
            Some(op) => {
                return Err(HomarrError::BadRequest(format!(
                    "Unknown firewall operation '{op}'"
                )))
            }
        },
        "vpn" => to_json_val(state.integrations.vpn_summary(&ctx).await?),
        "traefik" => to_json_val(state.integrations.traefik_dashboard(&ctx).await?),
        "network" => to_json_val(state.integrations.network_summary(&ctx).await?),
        "downloads" => {
            let limit = input
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 200) as usize;
            let selection: Option<DownloadClientSelection> = input
                .get("selection")
                .cloned()
                .filter(|v| !v.is_null())
                .map(serde_json::from_value)
                .transpose()
                .map_err(|err| HomarrError::BadRequest(format!("Invalid selection: {err}")))?;
            to_json_val(
                state
                    .integrations
                    .downloads_jobs_and_status(&ctx, limit, selection.as_ref())
                    .await?,
            )
        }
        "mediaReleases" => to_json_val(state.integrations.media_releases(&ctx).await?),
        "stats" => {
            let metrics = state.integrations.stats_metrics(ctx.kind);
            let values = state.integrations.stats_values(&ctx).await?;
            json!({ "metrics": metrics, "values": values })
        }
        "integrationData" => {
            let operation = body
                .operation
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    HomarrError::BadRequest("operation is required for integrationData".into())
                })?;
            if is_mutating_integration_operation(operation) {
                return Err(HomarrError::Forbidden(
                    "Mutating operations require the actions endpoint".into(),
                ));
            }
            state
                .integrations
                .integration_data(&ctx, operation, &input)
                .await?
        }
        other => {
            return Err(HomarrError::BadRequest(format!(
                "Unsupported query capability '{other}'"
            )))
        }
    };
    Ok(Json(json!({ "data": data })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationActionBody {
    pub action: String,
    #[serde(default)]
    pub input: Option<Value>,
}

fn to_json_val(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn parse_download_item(input: &Value) -> HomarrResult<DownloadClientItem> {
    let raw = input.get("item").unwrap_or(input).clone();
    serde_json::from_value(raw)
        .map_err(|err| HomarrError::BadRequest(format!("Invalid download item: {err}")))
}

fn parse_request_id(input: &Value) -> HomarrResult<i64> {
    let val = input
        .get("requestId")
        .or_else(|| input.get("id"))
        .ok_or_else(|| HomarrError::BadRequest("requestId is required".into()))?;
    if let Some(n) = val.as_i64() {
        return Ok(n);
    }
    if let Some(s) = val.as_str() {
        return s
            .trim()
            .parse::<i64>()
            .map_err(|_| HomarrError::BadRequest(format!("Invalid requestId '{s}'")));
    }
    Err(HomarrError::BadRequest("Invalid requestId".into()))
}

pub async fn execute_integration_action(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<IntegrationActionBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    check_integration_permission(&state.db, &session, &id, IntegrationPermission::Interact).await?;
    let ctx = load_ctx(&state, &id).await?;
    let input = body.input.unwrap_or(Value::Null);
    match body.action.trim() {
        "dnsHoleEnable" => state.integrations.dns_hole_enable(&ctx).await?,
        "dnsHoleDisable" => {
            let duration = input.get("duration").and_then(Value::as_u64);
            state.integrations.dns_hole_disable(&ctx, duration).await?;
        }
        "downloadsPauseQueue" => state.integrations.downloads_pause_queue(&ctx).await?,
        "downloadsResumeQueue" => state.integrations.downloads_resume_queue(&ctx).await?,
        "downloadsPauseItem" => {
            let item = parse_download_item(&input)?;
            state.integrations.downloads_pause_item(&ctx, &item).await?;
        }
        "downloadsResumeItem" => {
            let item = parse_download_item(&input)?;
            state
                .integrations
                .downloads_resume_item(&ctx, &item)
                .await?;
        }
        "downloadsDeleteItem" => {
            let item = parse_download_item(&input)?;
            let from_disk = input
                .get("fromDisk")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            state
                .integrations
                .downloads_delete_item(&ctx, &item, from_disk)
                .await?;
        }
        "indexersTestAll" => state.integrations.indexers_test_all(&ctx).await?,
        "mediaRequestApprove" => {
            let req_id = parse_request_id(&input)?;
            state
                .integrations
                .media_request_approve(&ctx, req_id)
                .await?;
        }
        "mediaRequestDecline" => {
            let req_id = parse_request_id(&input)?;
            state
                .integrations
                .media_request_decline(&ctx, req_id)
                .await?;
        }
        "smartHomeExecuteAction" => {
            let entity_id = input
                .get("entityId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| HomarrError::BadRequest("entityId is required".into()))?;
            let action_val = input
                .get("action")
                .cloned()
                .ok_or_else(|| HomarrError::BadRequest("action payload is required".into()))?;
            let smart_action: SmartHomeAction =
                serde_json::from_value(action_val).map_err(|err| {
                    HomarrError::BadRequest(format!("Invalid smart home action: {err}"))
                })?;
            let receipt = state
                .integrations
                .smart_home_execute_action(&ctx, entity_id, &smart_action)
                .await?;
            return Ok(Json(json!({ "ok": true, "receipt": receipt })));
        }
        "integrationData" => {
            let operation = input
                .get("operation")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    HomarrError::BadRequest(
                        "input.operation is required for integrationData".into(),
                    )
                })?;
            let result = state
                .integrations
                .integration_data(&ctx, operation, &input)
                .await?;
            return Ok(Json(json!({ "ok": true, "result": result })));
        }
        other => {
            return Err(HomarrError::BadRequest(format!(
                "Unsupported integration action '{other}'"
            )))
        }
    }
    Ok(Json(json!({ "ok": true })))
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
