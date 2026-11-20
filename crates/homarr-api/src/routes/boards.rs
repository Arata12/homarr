use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{can_view_board, check_board_permission, AuthSession, OptionalAuth, RequireAuth};
use homarr_common::grid_algorithm::{GridAlgorithmItem, GridItemType};
use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_db::{
    models::{
        FullBoard, GroupPermissionEntry, ItemLayoutRecord, ItemRecord, LayoutRecord,
        SectionLayoutRecord, UserPermissionEntry,
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
    section::{project_root_sections, LaneGeometry, RootSectionLike},
    BoardPermission, GroupPermissionKey, LayoutRole, WidgetKind,
};
use homarr_widgets::sanitize_widget_options;
use serde::{Deserialize, Serialize};
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

/// `boardNameSchema` from packages/validation/src/board.ts.
fn validate_board_name(name: &str) -> HomarrResult<()> {
    let valid = (1..=255).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(HomarrError::BadRequest(
            "Board name must be 1-255 characters of letters, digits, '-' or '_'".into(),
        ))
    }
}

/// `noBoardWithSimilarNameAsync`: names are unique case-insensitively.
async fn ensure_no_board_with_similar_name(
    db: &Database,
    name: &str,
    ignored_id: Option<&str>,
) -> HomarrResult<()> {
    let lower = name.to_lowercase();
    let taken = boards::list_all(db)
        .await?
        .iter()
        .any(|b| Some(b.id.as_str()) != ignored_id && b.name.to_lowercase() == lower);
    if taken {
        return Err(HomarrError::Conflict(
            "Board with similar name already exists".into(),
        ));
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
    let name = body.name;
    validate_board_name(&name)?;
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
        return Err(HomarrError::NotFound("Board not found".into()));
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
        return Err(HomarrError::NotFound("Board not found".into()));
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

    let new_name = body.name.as_deref();
    if let Some(name) = new_name {
        validate_board_name(name)?;
        ensure_no_board_with_similar_name(&state.db, name, Some(id.as_str())).await?;
    }
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
    let new_name = body.name.as_str();
    validate_board_name(new_name)?;
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
    ensure_no_board_with_similar_name(&state.db, new_name, None).await?;
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
        return Err(HomarrError::NotFound("Board not found".into()));
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
    let name = body.name.clone();
    validate_board_name(&name)?;
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
            sls.push(SectionLayoutRecord {
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
    let mut normalized = normalize_board_layout_roles(&inputs);
    let (sections, items) = project_layout_changes(&current, &mut normalized)?;
    let full =
        boards::replace_layouts_and_content(&state.db, &id, &normalized, &sections, &items).await?;
    Ok(Json(full))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutGeometry {
    column_count: i32,
    left_gutter_column_count: i32,
    right_gutter_column_count: i32,
}

impl LayoutGeometry {
    fn of_record(l: &LayoutRecord) -> Self {
        Self {
            column_count: l.column_count,
            left_gutter_column_count: l.left_gutter_column_count,
            right_gutter_column_count: l.right_gutter_column_count,
        }
    }

    fn of_input(l: &BoardLayoutNormInput) -> Self {
        Self {
            column_count: l.column_count,
            left_gutter_column_count: l.left_gutter_column_count,
            right_gutter_column_count: l.right_gutter_column_count,
        }
    }

    /// The original passes geometry without a role, so gutters always count.
    fn lanes(self) -> LaneGeometry {
        LaneGeometry {
            column_count: self.column_count,
            left_gutter_column_count: self.left_gutter_column_count,
            right_gutter_column_count: self.right_gutter_column_count,
            role: LayoutRole::Custom,
        }
    }
}

/// Stored `layout.resize_source` (packages/common/src/layout-resize-source.ts).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredResizeSource {
    geometry: LayoutGeometry,
    elements: Vec<GridAlgorithmItem>,
    projected_geometry: LayoutGeometry,
    projected_elements: Vec<GridAlgorithmItem>,
}

fn sorted_by_id(elements: &[GridAlgorithmItem]) -> Vec<GridAlgorithmItem> {
    let mut sorted = elements.to_vec();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    sorted
}

/// Keep the pre-resize arrangement only while the projected placements remain untouched.
fn get_layout_resize_source(
    geometry: LayoutGeometry,
    elements: Vec<GridAlgorithmItem>,
    serialized: Option<&str>,
) -> (LayoutGeometry, Vec<GridAlgorithmItem>) {
    if let Some(stored) =
        serialized.and_then(|raw| serde_json::from_str::<StoredResizeSource>(raw).ok())
    {
        if stored.projected_geometry == geometry
            && sorted_by_id(&stored.projected_elements) == sorted_by_id(&elements)
        {
            return (stored.geometry, stored.elements);
        }
    }
    (geometry, elements)
}

/// Placements of one layout as grid elements (`getElementsForLayout`).
fn layout_elements(board: &FullBoard, layout_id: &str) -> Vec<GridAlgorithmItem> {
    let items = board.items.iter().filter_map(|item| {
        let l = item.layouts.iter().find(|l| l.layout_id == layout_id)?;
        Some(GridAlgorithmItem {
            id: item.id.clone(),
            item_type: GridItemType::Item,
            width: l.width,
            height: l.height,
            x_offset: l.x_offset,
            y_offset: l.y_offset,
            section_id: l.section_id.clone(),
        })
    });
    let sections = board
        .sections
        .iter()
        .filter(|s| s.kind == "container")
        .filter_map(|s| {
            let l = s.layouts.iter().find(|l| l.layout_id == layout_id)?;
            Some(GridAlgorithmItem {
                id: s.id.clone(),
                item_type: GridItemType::Section,
                width: l.width,
                height: l.height,
                x_offset: l.x_offset,
                y_offset: l.y_offset,
                section_id: l.parent_section_id.clone()?,
            })
        });
    items.chain(sections).collect()
}

#[derive(Clone)]
struct ProjectedLayout {
    elements: Vec<GridAlgorithmItem>,
    resize_source: Option<String>,
}

/// `getUpdatedBoardLayout`: reflow `elements` from `previous` geometry into `current`.
fn project_layout(
    board: &FullBoard,
    previous: LayoutGeometry,
    elements: Vec<GridAlgorithmItem>,
    serialized_source: Option<&str>,
    current: LayoutGeometry,
) -> HomarrResult<ProjectedLayout> {
    let (source_geometry, source_elements) =
        get_layout_resize_source(previous, elements, serialized_source);
    let roots: Vec<RootSectionLike> = board
        .sections
        .iter()
        .map(|s| RootSectionLike {
            id: s.id.clone(),
            kind: s.kind.clone(),
            x_offset: s.x_offset,
            y_offset: s.y_offset,
        })
        .collect();
    let projected = project_root_sections(
        &roots,
        &source_elements,
        source_geometry.lanes(),
        current.lanes(),
    )
    .ok_or_else(|| HomarrError::Internal("Board has no main canvas root".into()))?;
    let resize_source = if source_geometry == current {
        None
    } else {
        serde_json::to_string(&StoredResizeSource {
            geometry: source_geometry,
            elements: source_elements,
            projected_geometry: current,
            projected_elements: projected.clone(),
        })
        .ok()
    };
    Ok(ProjectedLayout {
        elements: projected,
        resize_source,
    })
}

fn apply_projection(
    sections: &mut [SaveBoardSectionInput],
    items: &mut [SaveBoardItemInput],
    layout_id: &str,
    projected: &[GridAlgorithmItem],
) {
    for el in projected {
        match el.item_type {
            GridItemType::Item => {
                if let Some(item) = items.iter_mut().find(|i| i.id == el.id) {
                    item.layouts.retain(|l| l.layout_id != layout_id);
                    item.layouts.push(ItemLayoutRecord {
                        item_id: item.id.clone(),
                        section_id: el.section_id.clone(),
                        layout_id: layout_id.to_string(),
                        x_offset: el.x_offset,
                        y_offset: el.y_offset,
                        width: el.width,
                        height: el.height,
                    });
                }
            }
            GridItemType::Section => {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|s| s.id == el.id && s.kind == "container")
                {
                    section.layouts.retain(|l| l.layout_id != layout_id);
                    section.layouts.push(SectionLayoutRecord {
                        section_id: section.id.clone(),
                        layout_id: layout_id.to_string(),
                        parent_section_id: Some(el.section_id.clone()),
                        x_offset: el.x_offset,
                        y_offset: el.y_offset,
                        width: el.width,
                        height: el.height,
                    });
                }
            }
        }
    }
}

/// Placement half of `saveLayouts` (packages/api/src/router/board.ts): added layouts get the
/// (possibly resized) Base placements reflowed into their geometry, resized layouts reflow their
/// own placements, and placements of removed layouts are dropped. Sets the resize source of
/// every resized layout in `layouts`.
fn project_layout_changes(
    current: &FullBoard,
    layouts: &mut [BoardLayoutNormInput],
) -> HomarrResult<(Vec<SaveBoardSectionInput>, Vec<SaveBoardItemInput>)> {
    let kept: std::collections::HashSet<String> = layouts.iter().map(|l| l.id.clone()).collect();
    let mut sections = to_section_inputs(current);
    let mut items = to_item_inputs(current);
    for s in &mut sections {
        s.layouts.retain(|l| kept.contains(&l.layout_id));
    }
    for i in &mut items {
        i.layouts.retain(|l| kept.contains(&l.layout_id));
    }

    let base = current
        .layouts
        .iter()
        .find(|l| l.role == LayoutRole::Base.as_str())
        .ok_or_else(|| HomarrError::Internal("Board must have a Base layout".into()))?;
    let base_geometry = LayoutGeometry::of_record(base);
    let requested_base_geometry = layouts
        .iter()
        .find(|l| l.id == base.id)
        .map_or(base_geometry, LayoutGeometry::of_input);
    let resized_base = if requested_base_geometry == base_geometry {
        None
    } else {
        Some(project_layout(
            current,
            base_geometry,
            layout_elements(current, &base.id),
            base.resize_source.as_deref(),
            requested_base_geometry,
        )?)
    };
    let base_source_elements = match &resized_base {
        Some(p) => p.elements.clone(),
        None => layout_elements(current, &base.id),
    };

    for layout in layouts.iter_mut() {
        let target = LayoutGeometry::of_input(layout);
        let projection = match current.layouts.iter().find(|l| l.id == layout.id) {
            // Added layouts start without a resize source, like the original insert.
            None => project_layout(
                current,
                requested_base_geometry,
                base_source_elements.clone(),
                None,
                target,
            )?,
            Some(existing) => {
                let previous = LayoutGeometry::of_record(existing);
                if previous == target {
                    continue;
                }
                let projection = match &resized_base {
                    Some(p) if existing.id == base.id => p.clone(),
                    _ => project_layout(
                        current,
                        previous,
                        layout_elements(current, &existing.id),
                        existing.resize_source.as_deref(),
                        target,
                    )?,
                };
                layout.resize_source = projection.resize_source.clone();
                projection
            }
        };
        apply_projection(&mut sections, &mut items, &layout.id, &projection.elements);
    }
    Ok((sections, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_core::DatabaseDriver;

    #[tokio::test]
    async fn saving_layouts_projects_base_placements_into_added_and_resized_layouts() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let board = boards::create_board_with_defaults(
            &db,
            CreateBoardInput {
                id: None,
                name: "projection".into(),
                is_public: false,
                creator_id: None,
                column_count: 10,
            },
        )
        .await
        .unwrap();
        let base = board.layouts.iter().find(|l| l.role == "base").unwrap();
        let root = board.sections.iter().find(|s| s.kind == "empty").unwrap();
        let item = SaveBoardItemInput {
            id: "wide".into(),
            kind: "clock".into(),
            options: json!({}),
            advanced_options: json!({}),
            layouts: vec![ItemLayoutRecord {
                item_id: "wide".into(),
                section_id: root.id.clone(),
                layout_id: base.id.clone(),
                x_offset: 6,
                y_offset: 0,
                width: 4,
                height: 1,
            }],
            integration_ids: vec![],
        };
        let current =
            boards::save_board_content(&db, &board.board.id, &to_section_inputs(&board), &[item])
                .await
                .unwrap();

        let mut layouts: Vec<BoardLayoutNormInput> = current
            .layouts
            .iter()
            .map(|l| BoardLayoutNormInput {
                id: l.id.clone(),
                name: l.name.clone(),
                breakpoint: l.breakpoint,
                column_count: if l.id == base.id { 5 } else { l.column_count },
                left_gutter_column_count: 0,
                right_gutter_column_count: 0,
                role: LayoutRole::from_str_opt(&l.role),
                resize_source: None,
            })
            .collect();
        layouts.push(BoardLayoutNormInput {
            id: "added".into(),
            name: "Tablet".into(),
            breakpoint: 1200,
            column_count: 3,
            left_gutter_column_count: 0,
            right_gutter_column_count: 0,
            role: Some(LayoutRole::Custom),
            resize_source: None,
        });

        let (sections, items) = project_layout_changes(&current, &mut layouts).unwrap();
        let placement = |layout_id: &str| {
            items[0]
                .layouts
                .iter()
                .find(|l| l.layout_id == layout_id)
                .cloned()
                .unwrap()
        };
        let resized = placement(&base.id);
        assert!(resized.x_offset + resized.width <= 5, "{resized:?}");
        let added = placement("added");
        assert_eq!(added.section_id, root.id);
        assert!(added.x_offset + added.width <= 3, "{added:?}");
        let base_input = layouts.iter().find(|l| l.id == base.id).unwrap();
        assert!(base_input.resize_source.is_some());

        let saved =
            boards::replace_layouts_and_content(&db, &board.board.id, &layouts, &sections, &items)
                .await
                .unwrap();
        assert_eq!(saved.layouts.len(), 3);
        assert_eq!(saved.items[0].layouts.len(), 2);
    }
}
