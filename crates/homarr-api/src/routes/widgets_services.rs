use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    Json,
};
use homarr_auth::{
    check_integration_permission, AuthSession, OptionalAuth, RequireAdmin, RequireAuth,
};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::repos::custom_widgets::{self as cw_repo, CustomWidgetV2Input};
use homarr_definitions::{IntegrationPermission, WidgetKind};
use homarr_services::validate_custom_widget_definition;
use homarr_widgets::{all_widget_metadata, get_widget_metadata, render_widget_html};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn widget_catalog() -> Json<Value> {
    Json(json!({ "items": all_widget_metadata() }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderWidgetQuery {
    pub item_id: Option<String>,
    pub integration_ids: Option<String>,
    pub options: Option<String>,
}

/// Widget data endpoints take integration IDs from the caller, so each one must be usable by them.
/// Custom API widgets also read `options.integrationId` and run stored requests, so they need a session.
async fn authorize_integrations(
    state: &AppState,
    session: Option<&AuthSession>,
    kind: WidgetKind,
    options: &Value,
    integration_ids: &[String],
) -> HomarrResult<()> {
    let option_integration = options.get("integrationId").and_then(Value::as_str);
    if integration_ids.is_empty() && option_integration.is_none() && kind != WidgetKind::CustomApi {
        return Ok(());
    }
    let session = session.ok_or_else(|| {
        HomarrError::Unauthorized("Authentication required to use integrations".into())
    })?;
    for id in integration_ids
        .iter()
        .map(String::as_str)
        .chain(option_integration)
    {
        check_integration_permission(&state.db, session, id, IntegrationPermission::Use).await?;
    }
    Ok(())
}

pub async fn render_widget(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(kind_str): Path<String>,
    Query(q): Query<RenderWidgetQuery>,
) -> HomarrResult<Html<String>> {
    let kind = WidgetKind::from_str_opt(&kind_str)
        .ok_or_else(|| HomarrError::BadRequest(format!("Unknown widget kind '{kind_str}'")))?;
    let options: Value = q
        .options
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| get_widget_metadata(kind).default_options);
    let integration_ids: Vec<String> = q
        .integration_ids
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    authorize_integrations(&state, session.as_ref(), kind, &options, &integration_ids).await?;
    let data = state
        .widgets
        .resolve_widget_data(kind, &options, &integration_ids)
        .await?;
    let item_id = q.item_id.unwrap_or_else(|| "preview".to_string());
    Ok(Html(render_widget_html(kind, &item_id, &options, &data)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveWidgetBody {
    pub kind: String,
    #[serde(default)]
    pub options: Value,
    #[serde(default)]
    pub integration_ids: Vec<String>,
}

pub async fn resolve_widget(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Json(body): Json<ResolveWidgetBody>,
) -> HomarrResult<Json<Value>> {
    let kind = WidgetKind::from_str_opt(&body.kind)
        .ok_or_else(|| HomarrError::BadRequest(format!("Unknown widget kind '{}'", body.kind)))?;
    authorize_integrations(
        &state,
        session.as_ref(),
        kind,
        &body.options,
        &body.integration_ids,
    )
    .await?;
    let data = state
        .widgets
        .resolve_widget_data(kind, &body.options, &body.integration_ids)
        .await?;
    Ok(Json(data))
}

pub async fn list_custom_widgets(State(state): State<AppState>) -> HomarrResult<Json<Value>> {
    let items = cw_repo::list_v2(&state.db).await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn get_custom_widget(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    let item = cw_repo::get_v2_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Custom widget not found".into()))?;
    Ok(Json(serde_json::to_value(item).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomWidgetBody {
    #[serde(default = "default_cw_schema")]
    pub schema: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    #[serde(default)]
    pub sources: Value,
    #[serde(default)]
    pub requests: Value,
    #[serde(default)]
    pub options: Value,
    pub template: String,
    pub workshop: Option<Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub secrets: Vec<CwSecretInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CwSecretInput {
    pub source_id: String,
    pub kind: String,
    pub value: String,
}

fn default_cw_schema() -> String {
    "homarr-custom-widget-v2".to_string()
}

fn default_true() -> bool {
    true
}

pub async fn create_custom_widget(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CustomWidgetBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    let raw_def = json!({
        "schema": body.schema,
        "name": body.name,
        "template": body.template,
        "sources": body.sources,
        "requests": body.requests,
        "options": body.options,
    });
    validate_custom_widget_definition(&raw_def)?;

    let created = cw_repo::create_v2(
        &state.db,
        CustomWidgetV2Input {
            id: None,
            name: body.name,
            description: body.description,
            icon_url: body.icon_url,
            sources: body.sources,
            requests: body.requests,
            options: body.options,
            template: body.template,
            workshop: body.workshop,
            enabled: body.enabled,
            creator_id: Some(admin.user_id),
        },
    )
    .await?;

    for s in body.secrets {
        let enc = state.crypto.encrypt_secret(&s.value)?;
        cw_repo::upsert_v2_secret(&state.db, &created.id, &s.source_id, &s.kind, &enc).await?;
    }

    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(created).unwrap_or_default()),
    ))
}

pub async fn update_custom_widget(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CustomWidgetBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let raw_def = json!({
        "schema": body.schema,
        "name": body.name,
        "template": body.template,
        "sources": body.sources,
        "requests": body.requests,
        "options": body.options,
    });
    validate_custom_widget_definition(&raw_def)?;

    let updated = cw_repo::update_v2(
        &state.db,
        &id,
        CustomWidgetV2Input {
            id: None,
            name: body.name,
            description: body.description,
            icon_url: body.icon_url,
            sources: body.sources,
            requests: body.requests,
            options: body.options,
            template: body.template,
            workshop: body.workshop,
            enabled: body.enabled,
            creator_id: None,
        },
    )
    .await?;

    for s in body.secrets {
        let enc = state.crypto.encrypt_secret(&s.value)?;
        cw_repo::upsert_v2_secret(&state.db, &updated.id, &s.source_id, &s.kind, &enc).await?;
    }

    Ok(Json(serde_json::to_value(updated).unwrap_or_default()))
}

pub async fn delete_custom_widget(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let deleted = cw_repo::delete_v2(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Custom widget not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCwBody {
    pub request_id: String,
    #[serde(default)]
    pub options: HashMap<String, Value>,
}

pub async fn execute_custom_widget(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ExecuteCwBody>,
) -> HomarrResult<Json<Value>> {
    let result = state
        .services
        .custom_widgets
        .execute_request(
            &state.db,
            &state.crypto,
            &state.integrations,
            Some(&session),
            &id,
            &body.request_id,
            &body.options,
        )
        .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct WorkshopSearchQuery {
    pub q: Option<String>,
}

pub async fn workshop_listings(
    State(state): State<AppState>,
    Query(q): Query<WorkshopSearchQuery>,
) -> HomarrResult<Json<Value>> {
    let items = state
        .services
        .custom_widgets
        .fetch_workshop_listings(q.q.as_deref())
        .await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopInstallBody {
    pub listing_id: String,
}

pub async fn workshop_install(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<WorkshopInstallBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    let def = state
        .services
        .custom_widgets
        .fetch_workshop_definition(&body.listing_id)
        .await?;
    let created = cw_repo::create_v2(
        &state.db,
        CustomWidgetV2Input {
            id: None,
            name: def
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Workshop Widget")
                .to_string(),
            description: def
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            icon_url: def
                .get("iconUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            sources: def.get("sources").cloned().unwrap_or(json!([])),
            requests: def.get("requests").cloned().unwrap_or(json!([])),
            options: def.get("options").cloned().unwrap_or(json!([])),
            template: def
                .get("template")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            workshop: Some(json!({ "listingId": body.listing_id })),
            enabled: true,
            creator_id: Some(admin.user_id),
        },
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(created).unwrap_or_default()),
    ))
}

pub async fn docker_containers(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let items = state.services.docker.list_containers().await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn docker_container_action(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path((id, action)): Path<(String, String)>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    state.services.docker.container_action(&id, &action).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub tail: Option<usize>,
}

pub async fn docker_container_logs(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LogsQuery>,
) -> HomarrResult<Json<Value>> {
    let logs = state
        .services
        .docker
        .container_logs(&id, q.tail.unwrap_or(200))
        .await?;
    Ok(Json(json!({ "logs": logs })))
}

pub async fn docker_discover(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let items = state.services.docker.discover_services().await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn kubernetes_resource(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    Path(resource): Path<String>,
) -> HomarrResult<Json<Value>> {
    let data = state.services.kubernetes.get_resource(&resource).await?;
    Ok(Json(data))
}

#[derive(Deserialize)]
pub struct LatLonQuery {
    pub lat: f64,
    pub lon: f64,
}

pub async fn external_weather(
    State(state): State<AppState>,
    Query(q): Query<LatLonQuery>,
) -> HomarrResult<Json<Value>> {
    let data = state.services.external.get_weather(q.lat, q.lon).await?;
    Ok(Json(serde_json::to_value(data).unwrap_or_default()))
}

pub async fn external_air_quality(
    State(state): State<AppState>,
    Query(q): Query<LatLonQuery>,
) -> HomarrResult<Json<Value>> {
    let data = state
        .services
        .external
        .get_air_quality(q.lat, q.lon)
        .await?;
    Ok(Json(serde_json::to_value(data).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

pub async fn external_cities(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> HomarrResult<Json<Value>> {
    let items = state.services.external.search_cities(&q.q).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
pub struct RssQuery {
    pub url: String,
    pub limit: Option<usize>,
}

pub async fn external_rss(
    State(state): State<AppState>,
    Query(q): Query<RssQuery>,
) -> HomarrResult<Json<Value>> {
    let urls: Vec<String> = q
        .url
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let entries = state
        .services
        .external
        .fetch_feeds(&urls, q.limit.unwrap_or(10))
        .await?;
    Ok(Json(json!({ "items": entries })))
}

#[derive(Deserialize)]
pub struct StockQuery {
    pub symbol: String,
}

pub async fn external_stocks(
    State(state): State<AppState>,
    Query(q): Query<StockQuery>,
) -> HomarrResult<Json<Value>> {
    let info = state.services.external.get_stock_price(&q.symbol).await?;
    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct ReleasesQuery {
    pub provider: String,
    pub identifier: String,
}

pub async fn external_releases(
    State(state): State<AppState>,
    Query(q): Query<ReleasesQuery>,
) -> HomarrResult<Json<Value>> {
    let info = state
        .services
        .external
        .get_latest_release(&q.provider, &q.identifier)
        .await?;
    Ok(Json(serde_json::to_value(info).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct MinecraftQuery {
    pub host: String,
    pub port: Option<u16>,
}

pub async fn external_minecraft(
    State(state): State<AppState>,
    Query(q): Query<MinecraftQuery>,
) -> Json<Value> {
    let status = state
        .services
        .external
        .ping_minecraft_server(&q.host, q.port.unwrap_or(25565))
        .await;
    Json(serde_json::to_value(status).unwrap_or_default())
}
