use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{can_view_board, check_board_permission, AuthSession, OptionalAuth, RequireAuth};
use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_db::{
    models::{
        FullBoard, GroupPermissionEntry, ItemLayoutRecord, ItemRecord, LayoutRecord,
        UserPermissionEntry,
    },
    repos::{
        boards::{
            self, BoardSettingsPatch, CreateBoardInput, SaveBoardItemInput, SaveBoardSectionInput,
        },
        settings,
    },
    Database,
};
use homarr_definitions::{
    board::{normalize_board_layout_roles, BoardLayoutNormInput},
    BoardPermission, GroupPermissionKey, LayoutRole, WidgetKind,
};
use homarr_widgets::sanitize_widget_options;
use serde::Deserialize;
use serde_json::{json, Value};

use super::board_content::{
    normalize_legacy_sections, validate_board_content, validate_item_integrations,
};
use crate::state::AppState;

/// Custom widgets call integrations without the viewer's session when the board renders, so
/// only admins may add one or change its kind, options or integrations. Moving, resizing and
/// removing stay with the regular board permission. `submitted` must already be sanitized.
/// Stored options may predate option sanitizing; compare them in the same normalized form.
fn old_options(old: &ItemRecord) -> serde_json::Value {
    WidgetKind::from_str_opt(&old.kind)
        .map(|kind| sanitize_widget_options(kind, &old.options))
        .unwrap_or_else(|| old.options.clone())
}

fn ensure_custom_widget_changes_allowed(
    session: &AuthSession,
    submitted: &[SaveBoardItemInput],
    stored: &[ItemRecord],
) -> HomarrResult<()> {
    if session.is_admin() {
        return Ok(());
    }
    let custom = WidgetKind::CustomApi.as_str();
    for item in submitted {
        let forbidden = match stored.iter().find(|s| s.id == item.id) {
            None => item.kind == custom,
            Some(old) => {
                (old.kind == custom || item.kind == custom)
                    && (old.kind != item.kind
                        || old_options(old) != item.options
                        || old.integration_ids != item.integration_ids)
            }
        };
        if forbidden {
            return Err(HomarrError::Forbidden(
                "Only administrators can add or configure Custom Widgets".into(),
            ));
        }
    }
    Ok(())
}

fn to_section_inputs(full: &FullBoard) -> Vec<SaveBoardSectionInput> {
    full.sections
        .iter()
        .map(|s| SaveBoardSectionInput {
            id: s.id.clone(),
            kind: s.kind.clone(),
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            name: s.name.clone(),
            options: s.options.clone(),
            layouts: s.layouts.clone(),
        })
        .collect()
}

pub async fn list_boards(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let all = boards::list_all(&state.db).await?;
    let mut visible = Vec::new();
    for b in all {
        if can_view_board(&state.db, session.as_ref(), &b).await? {
            visible.push(b);
        }
    }
    Ok(Json(json!({ "items": visible })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardBody {
    pub name: String,
    #[serde(default)]
    pub is_public: bool,
    pub column_count: Option<i32>,
}

pub async fn create_board(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<CreateBoardBody>,
) -> HomarrResult<(StatusCode, Json<FullBoard>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::BoardCreate)?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(HomarrError::BadRequest("Board name is required".into()));
    }
    if boards::get_by_name(&state.db, &name).await?.is_some() {
        return Err(HomarrError::Conflict(
            "A board with that name already exists".into(),
        ));
    }
    let created = boards::create_board_with_defaults(
        &state.db,
        CreateBoardInput {
            id: None,
            name,
            is_public: body.is_public,
            creator_id: Some(session.user_id),
            column_count: body.column_count.unwrap_or(10),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

fn to_item_inputs(full: &FullBoard) -> Vec<SaveBoardItemInput> {
    full.items
        .iter()
        .map(|i| SaveBoardItemInput {
            id: i.id.clone(),
            kind: i.kind.clone(),
            options: i.options.clone(),
            advanced_options: i.advanced_options.clone(),
            layouts: i.layouts.clone(),
            integration_ids: i.integration_ids.clone(),
        })
        .collect()
}

async fn redact_permissions_unless_full(
    db: &Database,
    session: Option<&AuthSession>,
    full: &mut FullBoard,
) {
    let has_full = match session {
        Some(sess) => check_board_permission(db, Some(sess), &full.board, BoardPermission::Full)
            .await
            .is_ok(),
        None => false,
    };
    if !has_full {
        full.user_permissions.clear();
        full.group_permissions.clear();
    }
}

pub async fn get_board(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let mut full = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
    redact_permissions_unless_full(&state.db, session.as_ref(), &mut full).await;
    Ok(Json(full))
}

pub async fn get_board_by_name(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let mut full = boards::get_full_board_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
    redact_permissions_unless_full(&state.db, session.as_ref(), &mut full).await;
    Ok(Json(full))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchBoardBody {
    pub name: Option<String>,
    pub is_public: Option<bool>,
    #[serde(flatten)]
    pub settings: BoardSettingsPatch,
}

pub async fn update_board(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchBoardBody>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    // Renaming and changing visibility need full access, like the original.
    let required = if body.name.is_some() || body.is_public.is_some() {
        BoardPermission::Full
    } else {
        BoardPermission::Modify
    };
    check_board_permission(&state.db, Some(&session), &board, required).await?;

    let new_name = match body.name.as_deref().map(str::trim) {
        Some("") => {
            return Err(HomarrError::BadRequest("Board name cannot be empty".into()));
        }
        Some(trimmed) => {
            let lower = trimmed.to_lowercase();
            let taken = boards::list_all(&state.db)
                .await?
                .iter()
                .any(|b| b.id != id && b.name.to_lowercase() == lower);
            if taken {
                return Err(HomarrError::Conflict(
                    "Board with similar name already exists".into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };
    if body.is_public == Some(false) {
        let board_settings = settings::get_setting(&state.db, "board").await?;
        let is_home = ["homeBoardId", "mobileHomeBoardId"]
            .iter()
            .any(|k| board_settings.get(*k).and_then(Value::as_str) == Some(id.as_str()));
        if is_home {
            return Err(HomarrError::BadRequest(
                "Cannot make home board private".into(),
            ));
        }
    }
    if let Some(name) = new_name {
        boards::rename_board(&state.db, &id, name).await?;
    }
    if let Some(is_public) = body.is_public {
        boards::set_visibility(&state.db, &id, is_public).await?;
    }
    let _ = boards::update_settings(&state.db, &id, body.settings).await?;

    let updated = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    Ok(Json(updated))
}

pub async fn delete_board(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Full).await?;
    let deleted = boards::delete(&state.db, &id).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLayoutBody {
    pub sections: Vec<SaveBoardSectionInput>,
    pub items: Vec<SaveBoardItemInput>,
}

pub async fn save_board_layout(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut body): Json<SaveLayoutBody>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
    let current = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    normalize_legacy_sections(&mut body.sections);
    validate_board_content(&current.layouts, &body.sections, &body.items)?;
    validate_item_integrations(&state.db, &session, &body.items, &current.items).await?;
    for item in &mut body.items {
        if let Some(kind) = WidgetKind::from_str_opt(&item.kind) {
            item.options = sanitize_widget_options(kind, &item.options);
        }
    }
    ensure_custom_widget_changes_allowed(&session, &body.items, &current.items)?;
    let updated = boards::save_board_content(&state.db, &id, &body.sections, &body.items).await?;
    let _ = state
        .redis
        .publish_json(
            homarr_redis::RedisStore::BOARD_EVENTS_CHANNEL,
            &json!({ "boardId": id, "type": "layoutUpdated" }),
        )
        .await;
    Ok(Json(updated))
}

pub async fn create_board_item(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut item): Json<SaveBoardItemInput>,
) -> HomarrResult<(StatusCode, Json<FullBoard>)> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
    if item.id.trim().is_empty() {
        item.id = create_id();
    }
    let current = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    for l in &mut item.layouts {
        l.item_id = item.id.clone();
    }
    let section_inputs = to_section_inputs(&current);
    validate_board_content(
        &current.layouts,
        &section_inputs,
        std::slice::from_ref(&item),
    )?;
    validate_item_integrations(&state.db, &session, std::slice::from_ref(&item), &[]).await?;
    if let Some(kind) = WidgetKind::from_str_opt(&item.kind) {
        item.options = sanitize_widget_options(kind, &item.options);
    }
    ensure_custom_widget_changes_allowed(&session, std::slice::from_ref(&item), &[])?;
    boards::create_board_item(&state.db, &id, &item).await?;
    let updated = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    Ok((StatusCode::CREATED, Json(updated)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchItemBody {
    pub layout_id: Option<String>,
    pub section_id: Option<String>,
    pub x_offset: Option<i32>,
    pub y_offset: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub options: Option<Value>,
}

pub async fn update_board_item(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path((board_id, item_id)): Path<(String, String)>,
    Json(body): Json<PatchItemBody>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;

    let mut full = boards::get_full_board_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    let existing_items = full.items.clone();

    if body.layout_id.is_some() ^ body.section_id.is_some() {
        return Err(HomarrError::BadRequest(
            "Both layoutId and sectionId are required when updating placement".into(),
        ));
    }

    let mut found = false;
    let mut modified = false;
    for item in &mut full.items {
        if item.id == item_id {
            found = true;
            if let Some(opts) = body.options.clone() {
                item.options = if let Some(kind) = WidgetKind::from_str_opt(&item.kind) {
                    sanitize_widget_options(kind, &opts)
                } else {
                    opts
                };
                modified = true;
            }
            if let (Some(lid), Some(sid)) = (&body.layout_id, &body.section_id) {
                if let Some(existing_layout) = item.layouts.iter_mut().find(|l| l.layout_id == *lid)
                {
                    existing_layout.section_id = sid.clone();
                    if let Some(x) = body.x_offset {
                        existing_layout.x_offset = x;
                    }
                    if let Some(y) = body.y_offset {
                        existing_layout.y_offset = y;
                    }
                    if let Some(w) = body.width {
                        existing_layout.width = w;
                    }
                    if let Some(h) = body.height {
                        existing_layout.height = h;
                    }
                } else {
                    item.layouts.push(ItemLayoutRecord {
                        item_id: item_id.clone(),
                        section_id: sid.clone(),
                        layout_id: lid.clone(),
                        x_offset: body.x_offset.unwrap_or(0),
                        y_offset: body.y_offset.unwrap_or(0),
                        width: body.width.unwrap_or(1),
                        height: body.height.unwrap_or(1),
                    });
                }
                modified = true;
            }
        }
    }
    if !found {
        return Err(HomarrError::NotFound("Board item not found".into()));
    }
    if modified {
        let sections_in = to_section_inputs(&full);
        let items_in = to_item_inputs(&full);
        validate_board_content(&full.layouts, &sections_in, &items_in)?;
        validate_item_integrations(&state.db, &session, &items_in, &existing_items).await?;
        ensure_custom_widget_changes_allowed(&session, &items_in, &existing_items)?;
        full = boards::save_board_content(&state.db, &board_id, &sections_in, &items_in).await?;
    }
    Ok(Json(full))
}

pub async fn delete_board_item(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path((board_id, item_id)): Path<(String, String)>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;

    let full = boards::get_full_board_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    let sections_in: Vec<SaveBoardSectionInput> = full
        .sections
        .iter()
        .map(|s| SaveBoardSectionInput {
            id: s.id.clone(),
            kind: s.kind.clone(),
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            name: s.name.clone(),
            options: s.options.clone(),
            layouts: s.layouts.clone(),
        })
        .collect();
    let items_in: Vec<SaveBoardItemInput> = full
        .items
        .into_iter()
        .filter(|i| i.id != item_id)
        .map(|i| SaveBoardItemInput {
            id: i.id,
            kind: i.kind,
            options: i.options,
            advanced_options: i.advanced_options,
            layouts: i.layouts,
            integration_ids: i.integration_ids,
        })
        .collect();
    let updated = boards::save_board_content(&state.db, &board_id, &sections_in, &items_in).await?;
    Ok(Json(updated))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSectionBody {
    pub name: Option<String>,
    pub kind: Option<String>,
}

pub async fn create_board_section(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(board_id): Path<String>,
    Json(body): Json<AddSectionBody>,
) -> HomarrResult<(StatusCode, Json<FullBoard>)> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
    let full = boards::get_full_board_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;

    let next_y = full
        .sections
        .iter()
        .filter_map(|s| s.y_offset)
        .max()
        .map(|y| y + 1)
        .unwrap_or(0);

    let mut sections_in = to_section_inputs(&full);
    sections_in.push(SaveBoardSectionInput {
        id: create_id(),
        kind: body.kind.unwrap_or_else(|| "category".to_string()),
        x_offset: Some(0),
        y_offset: Some(next_y),
        name: Some(body.name.unwrap_or_else(|| "New Section".to_string())),
        options: json!({}),
        layouts: Vec::new(),
    });
    let items_in = to_item_inputs(&full);
    validate_board_content(&full.layouts, &sections_in, &items_in)?;
    let updated = boards::save_board_content(&state.db, &board_id, &sections_in, &items_in).await?;
    Ok((StatusCode::CREATED, Json(updated)))
}

pub async fn delete_board_section(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path((board_id, section_id)): Path<(String, String)>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
    let full = boards::get_full_board_by_id(&state.db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;

    let target = full
        .sections
        .iter()
        .find(|s| s.id == section_id)
        .cloned()
        .ok_or_else(|| HomarrError::NotFound("Section not found".into()))?;

    // Like the original container removal, child sections move up into the removed section's
    // parent (offsets shifted) per layout; children that cannot be re-parented are removed.
    let mut removed = std::collections::HashSet::from([section_id.clone()]);
    let mut sections_in = to_section_inputs(&full);
    for s in &mut sections_in {
        for l in &mut s.layouts {
            if l.parent_section_id.as_deref() != Some(section_id.as_str()) {
                continue;
            }
            let parent = target
                .layouts
                .iter()
                .find(|r| r.layout_id == l.layout_id && r.parent_section_id.is_some());
            match parent {
                Some(r) => {
                    l.parent_section_id = r.parent_section_id.clone();
                    l.x_offset += r.x_offset;
                    l.y_offset += r.y_offset;
                }
                None => {
                    removed.insert(s.id.clone());
                }
            }
        }
    }
    // Descendants of removed sections go too.
    loop {
        let before = removed.len();
        for s in &sections_in {
            let orphan = s.layouts.iter().any(|l| {
                l.parent_section_id
                    .as_ref()
                    .is_some_and(|p| removed.contains(p))
            });
            if orphan {
                removed.insert(s.id.clone());
            }
        }
        if removed.len() == before {
            break;
        }
    }
    sections_in.retain(|s| !removed.contains(&s.id));
    if sections_in.is_empty() {
        return Err(HomarrError::BadRequest(
            "A board must have at least one section".into(),
        ));
    }

    // Only the placements inside removed sections go; an item is deleted only when that was
    // its last placement.
    let mut items_in = to_item_inputs(&full);
    items_in.retain_mut(|i| {
        let had_placements = !i.layouts.is_empty();
        i.layouts.retain(|l| !removed.contains(&l.section_id));
        !had_placements || !i.layouts.is_empty()
    });
    validate_board_content(&full.layouts, &sections_in, &items_in)?;
    let updated = boards::save_board_content(&state.db, &board_id, &sections_in, &items_in).await?;
    Ok(Json(updated))
}

pub async fn get_board_permissions(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Full).await?;
    let (users, groups) = boards::get_permissions(&state.db, &id).await?;
    Ok(Json(json!({ "users": users, "groups": groups })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPermissionsBody {
    pub users: Option<Vec<UserPermissionEntry>>,
    pub groups: Option<Vec<GroupPermissionEntry>>,
}

pub async fn set_board_permissions(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetPermissionsBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Full).await?;

    for u in body.users.iter().flatten() {
        if BoardPermission::from_str_opt(&u.permission).is_none() {
            return Err(HomarrError::BadRequest(format!(
                "Invalid board permission '{}'",
                u.permission
            )));
        }
    }
    for g in body.groups.iter().flatten() {
        if BoardPermission::from_str_opt(&g.permission).is_none() {
            return Err(HomarrError::BadRequest(format!(
                "Invalid board permission '{}'",
                g.permission
            )));
        }
    }

    let (existing_users, existing_groups) = if body.users.is_none() || body.groups.is_none() {
        boards::get_permissions(&state.db, &id).await?
    } else {
        (Vec::new(), Vec::new())
    };
    let users = body.users.unwrap_or(existing_users);
    let groups = body.groups.unwrap_or(existing_groups);
    boards::set_permissions(&state.db, &id, &users, &groups).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct DuplicateBoardBody {
    pub name: String,
}

pub async fn duplicate_board(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<DuplicateBoardBody>,
) -> HomarrResult<(StatusCode, Json<FullBoard>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::BoardCreate)?;
    let new_name = body.name.trim();
    if new_name.is_empty() {
        return Err(HomarrError::BadRequest("Board name is required".into()));
    }
    let source = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(
        &state.db,
        Some(&session),
        &source.board,
        BoardPermission::View,
    )
    .await?;
    let source_items = to_item_inputs(&source);
    validate_item_integrations(&state.db, &session, &source_items, &[]).await?;
    ensure_custom_widget_changes_allowed(&session, &source_items, &[])?;
    let dup = boards::duplicate_board(&state.db, &id, new_name, Some(&session.user_id)).await?;
    Ok((StatusCode::CREATED, Json(dup)))
}

pub async fn export_board(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let mut full = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
    redact_permissions_unless_full(&state.db, session.as_ref(), &mut full).await;
    Ok(Json(full))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBoardBody {
    pub name: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub layouts: Vec<LayoutInputBody>,
    #[serde(default)]
    pub sections: Vec<SaveBoardSectionInput>,
    #[serde(default)]
    pub items: Vec<SaveBoardItemInput>,
}

pub async fn import_board(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<ImportBoardBody>,
) -> HomarrResult<(StatusCode, Json<FullBoard>)> {
    session.require_writable(state.demo_read_only_active())?;
    session.require_permission(GroupPermissionKey::BoardCreate)?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(HomarrError::BadRequest("Board name is required".into()));
    }
    if boards::get_by_name(&state.db, &name).await?.is_some() {
        return Err(HomarrError::Conflict(
            "A board with that name already exists".into(),
        ));
    }

    // Build layout mappings & validate content BEFORE writing to DB.
    let mut layout_id_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let norm_layouts: Vec<BoardLayoutNormInput> = if !body.layouts.is_empty() {
        if body.layouts.len() > 12 {
            return Err(HomarrError::BadRequest(
                "A board needs between 1 and 12 layouts".into(),
            ));
        }
        let mut raw_inputs = Vec::with_capacity(body.layouts.len());
        for l in &body.layouts {
            let lname = l.name.trim();
            if lname.is_empty() || lname.chars().count() > 32 {
                return Err(HomarrError::BadRequest(
                    "Layout name must be 1-32 characters".into(),
                ));
            }
            if !(1..=24).contains(&l.column_count) {
                return Err(HomarrError::BadRequest(
                    "Column count must be between 1 and 24".into(),
                ));
            }
            if !(0..=32_767).contains(&l.breakpoint) {
                return Err(HomarrError::BadRequest("Invalid breakpoint".into()));
            }
            if l.left_gutter_column_count < 0
                || l.right_gutter_column_count < 0
                || (l.left_gutter_column_count + l.right_gutter_column_count
                    >= l.column_count.max(1)
                    && l.left_gutter_column_count + l.right_gutter_column_count > 0)
            {
                return Err(HomarrError::BadRequest(
                    "Invalid gutter column count".into(),
                ));
            }
            let new_lid = create_id();
            if let Some(old_lid) = &l.id {
                layout_id_map.insert(old_lid.clone(), new_lid.clone());
            }
            raw_inputs.push(BoardLayoutNormInput {
                id: new_lid,
                name: lname.to_string(),
                breakpoint: l.breakpoint,
                column_count: l.column_count,
                left_gutter_column_count: l.left_gutter_column_count,
                right_gutter_column_count: l.right_gutter_column_count,
                role: None,
                resize_source: None,
            });
        }
        normalize_board_layout_roles(&raw_inputs)
    } else {
        let base_id = create_id();
        let mobile_id = create_id();
        let mut distinct_refs: Vec<String> = Vec::new();
        for s in &body.sections {
            for sl in &s.layouts {
                if !distinct_refs.contains(&sl.layout_id) {
                    distinct_refs.push(sl.layout_id.clone());
                }
            }
        }
        for it in &body.items {
            for il in &it.layouts {
                if !distinct_refs.contains(&il.layout_id) {
                    distinct_refs.push(il.layout_id.clone());
                }
            }
        }
        if distinct_refs.len() > 1 {
            return Err(HomarrError::BadRequest(
                "Import referencing multiple layout IDs must include layouts".into(),
            ));
        }
        if let Some(only_ref) = distinct_refs.into_iter().next() {
            layout_id_map.insert(only_ref, base_id.clone());
        }
        vec![
            BoardLayoutNormInput {
                id: base_id,
                name: "Base".into(),
                breakpoint: 0,
                column_count: 10,
                left_gutter_column_count: 0,
                right_gutter_column_count: 0,
                role: Some(LayoutRole::Base),
                resize_source: None,
            },
            BoardLayoutNormInput {
                id: mobile_id,
                name: "Mobile".into(),
                breakpoint: 0,
                column_count: 1,
                left_gutter_column_count: 0,
                right_gutter_column_count: 0,
                role: Some(LayoutRole::Mobile),
                resize_source: None,
            },
        ]
    };

    let layout_records: Vec<LayoutRecord> = norm_layouts
        .iter()
        .map(|l| LayoutRecord {
            id: l.id.clone(),
            name: l.name.clone(),
            board_id: String::new(),
            column_count: l.column_count,
            left_gutter_column_count: l.left_gutter_column_count,
            right_gutter_column_count: l.right_gutter_column_count,
            breakpoint: l.breakpoint,
            role: l.role.unwrap_or(LayoutRole::Custom).as_str().to_string(),
            resize_source: l.resize_source.clone(),
        })
        .collect();

    let mut section_id_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for s in &body.sections {
        if s.id.trim().is_empty() {
            return Err(HomarrError::BadRequest("Invalid section id".into()));
        }
        if section_id_map.contains_key(&s.id) {
            return Err(HomarrError::BadRequest("Duplicate section id".into()));
        }
        section_id_map.insert(s.id.clone(), create_id());
    }

    let mut remapped_sections = Vec::with_capacity(body.sections.len());
    for s in &body.sections {
        let new_sid = section_id_map[&s.id].clone();
        let mut sls = Vec::with_capacity(s.layouts.len());
        for sl in &s.layouts {
            let new_lid = layout_id_map
                .get(&sl.layout_id)
                .ok_or_else(|| {
                    HomarrError::BadRequest("Section references an unknown layout".into())
                })?
                .clone();
            let new_pid = match &sl.parent_section_id {
                Some(pid) => Some(
                    section_id_map
                        .get(pid)
                        .ok_or_else(|| {
                            HomarrError::BadRequest(
                                "Section references an invalid parent section".into(),
                            )
                        })?
                        .clone(),
                ),
                None => None,
            };
            sls.push(homarr_db::models::SectionLayoutRecord {
                section_id: new_sid.clone(),
                layout_id: new_lid,
                parent_section_id: new_pid,
                x_offset: sl.x_offset,
                y_offset: sl.y_offset,
                width: sl.width,
                height: sl.height,
            });
        }
        remapped_sections.push(SaveBoardSectionInput {
            id: new_sid,
            kind: s.kind.clone(),
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            name: s.name.clone(),
            options: s.options.clone(),
            layouts: sls,
        });
    }

    let mut seen_old_items = std::collections::HashSet::new();
    let mut remapped_items = Vec::with_capacity(body.items.len());
    for item in &body.items {
        if !item.id.trim().is_empty() && !seen_old_items.insert(item.id.as_str()) {
            return Err(HomarrError::BadRequest("Duplicate item id".into()));
        }
        let new_iid = create_id();
        let mut ils = Vec::with_capacity(item.layouts.len());
        for il in &item.layouts {
            let new_sid = section_id_map
                .get(&il.section_id)
                .ok_or_else(|| {
                    HomarrError::BadRequest("Item references an unknown section".into())
                })?
                .clone();
            let new_lid = layout_id_map
                .get(&il.layout_id)
                .ok_or_else(|| HomarrError::BadRequest("Item references an unknown layout".into()))?
                .clone();
            ils.push(ItemLayoutRecord {
                item_id: new_iid.clone(),
                section_id: new_sid,
                layout_id: new_lid,
                x_offset: il.x_offset,
                y_offset: il.y_offset,
                width: il.width,
                height: il.height,
            });
        }
        let sanitized_opts = if let Some(kind) = WidgetKind::from_str_opt(&item.kind) {
            sanitize_widget_options(kind, &item.options)
        } else {
            item.options.clone()
        };
        remapped_items.push(SaveBoardItemInput {
            id: new_iid,
            kind: item.kind.clone(),
            options: sanitized_opts,
            advanced_options: item.advanced_options.clone(),
            layouts: ils,
            integration_ids: item.integration_ids.clone(),
        });
    }

    normalize_legacy_sections(&mut remapped_sections);
    validate_board_content(&layout_records, &remapped_sections, &remapped_items)?;
    validate_item_integrations(&state.db, &session, &remapped_items, &[]).await?;
    ensure_custom_widget_changes_allowed(&session, &remapped_items, &[])?;

    let created = boards::create_board_with_defaults(
        &state.db,
        CreateBoardInput {
            id: None,
            name,
            is_public: body.is_public,
            creator_id: Some(session.user_id),
            column_count: 10,
        },
    )
    .await?;
    let persist_res = async {
        let _ = settings::get_setting(&state.db, "board").await?;
        boards::replace_layouts(&state.db, &created.board.id, &norm_layouts).await?;
        boards::save_board_content(
            &state.db,
            &created.board.id,
            &remapped_sections,
            &remapped_items,
        )
        .await
    }
    .await;
    match persist_res {
        Ok(updated) => Ok((StatusCode::CREATED, Json(updated))),
        Err(err) => {
            let _ = boards::delete(&state.db, &created.board.id).await;
            Err(err)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInputBody {
    pub id: Option<String>,
    pub name: String,
    pub breakpoint: i32,
    pub column_count: i32,
    #[serde(default)]
    pub left_gutter_column_count: i32,
    #[serde(default)]
    pub right_gutter_column_count: i32,
}

#[derive(Deserialize)]
pub struct SetLayoutsBody {
    pub layouts: Vec<LayoutInputBody>,
}

/// Replaces the responsive layouts (name, breakpoint, column count) of a board. The mobile
/// and base layouts cannot be removed; items that no longer fit are clamped to the new width.
pub async fn set_board_layouts(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetLayoutsBody>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
    let current = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;

    if body.layouts.is_empty() || body.layouts.len() > 12 {
        return Err(HomarrError::BadRequest(
            "A board needs between 1 and 12 layouts".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut inputs = Vec::new();
    for l in &body.layouts {
        let name = l.name.trim();
        if name.is_empty() || name.chars().count() > 32 {
            return Err(HomarrError::BadRequest(
                "Layout name must be 1-32 characters".into(),
            ));
        }
        if !(1..=24).contains(&l.column_count) {
            return Err(HomarrError::BadRequest(
                "Column count must be between 1 and 24".into(),
            ));
        }
        if !(0..=32_767).contains(&l.breakpoint) {
            return Err(HomarrError::BadRequest("Invalid breakpoint".into()));
        }
        if l.left_gutter_column_count < 0
            || l.right_gutter_column_count < 0
            || l.left_gutter_column_count + l.right_gutter_column_count >= l.column_count.max(1)
                && l.left_gutter_column_count + l.right_gutter_column_count > 0
        {
            return Err(HomarrError::BadRequest(
                "Invalid gutter column count".into(),
            ));
        }
        let existing =
            l.id.as_deref()
                .and_then(|lid| current.layouts.iter().find(|c| c.id == lid));
        let layout_id = match (&l.id, existing) {
            (Some(_), Some(e)) => e.id.clone(),
            (Some(_), None) => {
                return Err(HomarrError::BadRequest("Unknown layout id".into()));
            }
            (None, _) => create_id(),
        };
        if !seen.insert(layout_id.clone()) {
            return Err(HomarrError::BadRequest("Duplicate layout id".into()));
        }
        inputs.push(BoardLayoutNormInput {
            id: layout_id,
            name: name.to_string(),
            breakpoint: l.breakpoint,
            column_count: l.column_count,
            left_gutter_column_count: l.left_gutter_column_count,
            right_gutter_column_count: l.right_gutter_column_count,
            role: Some(
                existing
                    .and_then(|e| LayoutRole::from_str_opt(&e.role))
                    .unwrap_or(LayoutRole::Custom),
            ),
            resize_source: existing.and_then(|e| e.resize_source.clone()),
        });
    }
    for protected in current.layouts.iter().filter(|l| l.role != "custom") {
        if !seen.contains(&protected.id) {
            return Err(HomarrError::BadRequest(
                "The base and mobile layouts cannot be removed".into(),
            ));
        }
    }
    let normalized = normalize_board_layout_roles(&inputs);
    let _ = boards::replace_layouts(&state.db, &id, &normalized).await?;

    // Clamp placements to the (possibly reduced) column counts.
    let mut full = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    let cols: std::collections::HashMap<String, i32> = full
        .layouts
        .iter()
        .map(|l| (l.id.clone(), l.column_count))
        .collect();
    let mut changed = false;
    for item in &mut full.items {
        for pl in &mut item.layouts {
            if let Some(&c) = cols.get(&pl.layout_id) {
                if pl.x_offset + pl.width > c {
                    pl.width = pl.width.min(c);
                    pl.x_offset = (c - pl.width).max(0);
                    changed = true;
                }
            }
        }
    }
    if changed {
        let sections = to_section_inputs(&full);
        let items: Vec<SaveBoardItemInput> = full
            .items
            .iter()
            .map(|i| SaveBoardItemInput {
                id: i.id.clone(),
                kind: i.kind.clone(),
                options: i.options.clone(),
                advanced_options: i.advanced_options.clone(),
                layouts: i.layouts.clone(),
                integration_ids: i.integration_ids.clone(),
            })
            .collect();
        full = boards::save_board_content(&state.db, &id, &sections, &items).await?;
    }
    Ok(Json(full))
}
