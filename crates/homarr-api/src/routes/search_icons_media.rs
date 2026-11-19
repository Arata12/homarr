use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use homarr_auth::RequireAuth;
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::repos::{
    icons, medias,
    search_engines::{self, SearchEngineInput},
};
use homarr_definitions::GroupPermissionKey;
use homarr_services::{image_proxy::is_image_content_type, ImageProxyResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn list_search_engines(State(state): State<AppState>) -> HomarrResult<Json<Value>> {
    let items = search_engines::list_all(&state.db).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEngineBody {
    pub icon_url: String,
    pub name: String,
    pub short: String,
    pub description: Option<String>,
    pub url_template: Option<String>,
    #[serde(rename = "type", default = "default_se_kind")]
    pub kind: String,
    pub integration_id: Option<String>,
}

fn default_se_kind() -> String {
    "generic".to_string()
}

pub async fn create_search_engine(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<SearchEngineBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::SearchEngineCreate)?;
    let item = search_engines::create(
        &state.db,
        SearchEngineInput {
            id: None,
            icon_url: body.icon_url,
            name: body.name,
            short: body.short,
            description: body.description,
            url_template: body.url_template,
            kind: body.kind,
            integration_id: body.integration_id,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "searchEngine": item }))))
}

pub async fn update_search_engine(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SearchEngineBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::SearchEngineModifyAll)?;
    let item = search_engines::update(
        &state.db,
        &id,
        SearchEngineInput {
            id: None,
            icon_url: body.icon_url,
            name: body.name,
            short: body.short,
            description: body.description,
            url_template: body.url_template,
            kind: body.kind,
            integration_id: body.integration_id,
        },
    )
    .await?;
    Ok(Json(json!({ "searchEngine": item })))
}

pub async fn delete_search_engine(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::SearchEngineFullAll)?;
    let deleted = search_engines::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Search engine not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct DdgQuery {
    pub q: Option<String>,
}

pub async fn ddg_bangs(
    State(state): State<AppState>,
    Query(params): Query<DdgQuery>,
) -> HomarrResult<Json<Value>> {
    let mut bangs = state.services.external.get_ddg_bangs().await?;
    if let Some(q) = params.q.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        if !q.is_empty() {
            bangs.retain(|b| b.t.starts_with(&q) || b.s.to_ascii_lowercase().contains(&q));
        }
    }
    bangs.truncate(50);
    Ok(Json(json!({ "items": bangs })))
}

#[derive(Deserialize)]
pub struct IconsQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn search_icons(
    State(state): State<AppState>,
    Query(params): Query<IconsQuery>,
) -> HomarrResult<Json<Value>> {
    let limit = params.limit.unwrap_or(60).clamp(1, 200);
    let offset = params.offset.unwrap_or(0).max(0);
    let (items, total) =
        icons::search_icons(&state.db, params.q.as_deref().unwrap_or(""), limit, offset).await?;
    let repos = icons::list_repositories(&state.db).await?;
    Ok(Json(json!({
        "items": items,
        "total": total,
        "repositories": repos
    })))
}

pub async fn sync_icons(
    homarr_auth::RequireAdmin(admin): homarr_auth::RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let summary = state.services.icons.sync_all(&state.db).await?;
    Ok(Json(serde_json::to_value(summary).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct FaviconQuery {
    pub url: String,
}

pub async fn fetch_favicon(
    State(state): State<AppState>,
    Query(params): Query<FaviconQuery>,
) -> HomarrResult<Json<Value>> {
    let icon_url = state.services.icons.fetch_favicon_url(&params.url).await?;
    Ok(Json(json!({ "iconUrl": icon_url })))
}

pub async fn list_medias(State(state): State<AppState>) -> HomarrResult<Json<Value>> {
    let items = medias::list_metadata(&state.db).await?;
    Ok(Json(json!({ "items": items })))
}

pub async fn upload_media(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::MediaUpload)?;

    if let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| HomarrError::BadRequest(format!("Multipart error: {e}")))?
    {
        let file_name = field
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "upload.bin".to_string());
        let content_type = field
            .content_type()
            .map(str::to_string)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        if !matches!(
            content_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/x-icon"
        ) {
            return Err(HomarrError::BadRequest(format!(
                "Unsupported media content-type '{content_type}'"
            )));
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|e| HomarrError::BadRequest(format!("Read upload failed: {e}")))?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(HomarrError::BadRequest(
                "Media upload exceeds 32MB limit".into(),
            ));
        }
        let summary = medias::create(
            &state.db,
            &file_name,
            &bytes,
            &content_type,
            Some(&session.user_id),
        )
        .await?;
        return Ok((StatusCode::CREATED, Json(json!({ "media": summary }))));
    }

    Err(HomarrError::BadRequest("No file field provided".into()))
}

pub async fn get_media(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Response> {
    let media = medias::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Media not found".into()))?;
    let mut resp = Response::new(Body::from(media.content));
    *resp.status_mut() = StatusCode::OK;
    if let Ok(ct) = HeaderValue::from_str(&media.content_type) {
        resp.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(resp)
}

pub async fn delete_media(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::MediaFullAll)?;
    let deleted = medias::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Media not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

pub async fn image_proxy_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Response> {
    match state.services.image_proxy.resolve_image(&id).await? {
        ImageProxyResult::Redirect(target) => Ok(Redirect::temporary(&target).into_response()),
        ImageProxyResult::Bytes { content_type, data } => {
            let mut resp = Response::new(Body::from(data));
            *resp.status_mut() = StatusCode::OK;
            let headers = resp.headers_mut();
            // Upstream bytes are untrusted: only image/* passes through, and the sandbox CSP
            // keeps SVG/HTML from ever executing as a document on Homarr's origin.
            let ct = HeaderValue::from_str(&content_type)
                .ok()
                .filter(|_| is_image_content_type(&content_type))
                .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"));
            headers.insert(header::CONTENT_TYPE, ct);
            headers.insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("sandbox; default-src 'none'; style-src 'unsafe-inline'"),
            );
            headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=3600"),
            );
            Ok(resp)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardImageBody {
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

pub async fn image_proxy_forward(
    homarr_auth::RequireAdmin(admin): homarr_auth::RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<ForwardImageBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let proxied_url = state
        .services
        .image_proxy
        .forward_image(&body.url, body.headers)
        .await?;
    Ok(Json(json!({ "url": proxied_url })))
}
