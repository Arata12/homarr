use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use homarr_auth::{can_view_board, check_board_permission, OptionalAuth, RequireAuth};
use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_db::{
    models::{FullBoard, GroupPermissionEntry, UserPermissionEntry},
    repos::{
        boards::{
            self, BoardSettingsPatch, CreateBoardInput, SaveBoardItemInput, SaveBoardSectionInput,
        },
        settings,
    },
};
use homarr_definitions::{BoardPermission, GroupPermissionKey};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

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

pub async fn get_board(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let full = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
    Ok(Json(full))
}

pub async fn get_board_by_name(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let full = boards::get_full_board_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
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
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;

    if let Some(ref name) = body.name {
        boards::rename_board(&state.db, &id, name.trim()).await?;
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
    Json(body): Json<SaveLayoutBody>,
) -> HomarrResult<Json<FullBoard>> {
    session.require_writable(state.demo_read_only_active())?;
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::Modify).await?;
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

    let mut modified = false;
    for item in &mut full.items {
        if item.id == item_id {
            if let Some(opts) = body.options.clone() {
                item.options = opts;
                modified = true;
            }
            if let (Some(lid), Some(sid)) = (&body.layout_id, &body.section_id) {
                boards::update_item_layout(
                    &state.db,
                    &item_id,
                    lid,
                    sid,
                    body.x_offset.unwrap_or(0),
                    body.y_offset.unwrap_or(0),
                    body.width.unwrap_or(1),
                    body.height.unwrap_or(1),
                )
                .await?;
            }
        }
    }
    if modified {
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
        full = boards::save_board_content(&state.db, &board_id, &sections_in, &items_in).await?;
    } else {
        full = boards::get_full_board_by_id(&state.db, &board_id)
            .await?
            .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
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

    let mut sections_in: Vec<SaveBoardSectionInput> = full
        .sections
        .into_iter()
        .map(|s| SaveBoardSectionInput {
            id: s.id,
            kind: s.kind,
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            name: s.name,
            options: s.options,
            layouts: s.layouts,
        })
        .collect();
    sections_in.push(SaveBoardSectionInput {
        id: create_id(),
        kind: body.kind.unwrap_or_else(|| "category".to_string()),
        x_offset: Some(0),
        y_offset: Some(next_y),
        name: Some(body.name.unwrap_or_else(|| "New Section".to_string())),
        options: json!({}),
        layouts: Vec::new(),
    });
    let items_in: Vec<SaveBoardItemInput> = full
        .items
        .into_iter()
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

    let sections_in: Vec<SaveBoardSectionInput> = full
        .sections
        .into_iter()
        .filter(|s| s.id != section_id)
        .map(|s| SaveBoardSectionInput {
            id: s.id,
            kind: s.kind,
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            name: s.name,
            options: s.options,
            layouts: s.layouts,
        })
        .collect();
    let items_in: Vec<SaveBoardItemInput> = full
        .items
        .into_iter()
        .filter(|i| !i.layouts.iter().any(|l| l.section_id == section_id))
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
    #[serde(default)]
    pub users: Vec<UserPermissionEntry>,
    #[serde(default)]
    pub groups: Vec<GroupPermissionEntry>,
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
    boards::set_permissions(&state.db, &id, &body.users, &body.groups).await?;
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
    let board = boards::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    check_board_permission(&state.db, Some(&session), &board, BoardPermission::View).await?;
    let dup =
        boards::duplicate_board(&state.db, &id, body.name.trim(), Some(&session.user_id)).await?;
    Ok((StatusCode::CREATED, Json(dup)))
}

pub async fn export_board(
    OptionalAuth(session): OptionalAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<FullBoard>> {
    let full = boards::get_full_board_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
    if !can_view_board(&state.db, session.as_ref(), &full.board).await? {
        return Err(HomarrError::Forbidden("Board access denied".into()));
    }
    Ok(Json(full))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBoardBody {
    pub name: String,
    #[serde(default)]
    pub is_public: bool,
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
    let created = boards::create_board_with_defaults(
        &state.db,
        CreateBoardInput {
            id: None,
            name: body.name.trim().to_string(),
            is_public: body.is_public,
            creator_id: Some(session.user_id),
            column_count: 10,
        },
    )
    .await?;
    let _ = settings::get_setting(&state.db, "board").await?;
    let updated =
        boards::save_board_content(&state.db, &created.board.id, &body.sections, &body.items)
            .await?;
    Ok((StatusCode::CREATED, Json(updated)))
}
