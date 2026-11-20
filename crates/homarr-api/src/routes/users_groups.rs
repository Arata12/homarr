use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use homarr_auth::{can_view_board, hash_password, verify_password, RequireAdmin, RequireAuth};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    models::GroupRecord,
    repos::{
        boards, groups, invites,
        users::{self, CreateUserInput, UpdateUserPreferencesInput},
    },
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::auth::throw_if_credentials_disabled;
use crate::state::AppState;

/// Built-in group every user belongs to; it cannot be renamed, deleted or have members edited.
const EVERYONE_GROUP: &str = "everyone";

pub async fn list_users(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let all = users::list_all(&state.db).await?;
    Ok(Json(json!({ "items": all })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserBody {
    pub name: String,
    pub password: String,
    pub email: Option<String>,
}

pub async fn create_user(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CreateUserBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    throw_if_credentials_disabled(&state)?;
    let name = body.name.trim().to_string();
    if name.len() < 2 || body.password.chars().count() < 8 {
        return Err(HomarrError::BadRequest(
            "Username must be >= 2 chars and password >= 8 chars".into(),
        ));
    }
    if users::find_by_name_ci(&state.db, &name).await?.is_some() {
        return Err(HomarrError::Conflict("Username already exists".into()));
    }
    let password_hash = hash_password(&body.password).await?;
    let user = users::create(
        &state.db,
        CreateUserInput {
            id: None,
            name,
            email: body.email,
            password_hash: Some(password_hash),
            provider: "credentials".to_string(),
            image: None,
        },
    )
    .await?;
    if let Some(everyone) = groups::get_by_name(&state.db, "everyone").await? {
        let _ = groups::add_member(&state.db, &everyone.id, &user.id).await;
    }
    Ok((StatusCode::CREATED, Json(json!({ "user": user }))))
}

pub async fn get_user(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    if !session.is_admin() && session.user_id != id {
        return Err(HomarrError::Forbidden("Forbidden".into()));
    }
    let user = users::find_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("User not found".into()))?;
    Ok(Json(json!({ "user": user })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserBody {
    pub name: Option<String>,
    pub email: Option<String>,
    pub image: Option<String>,
    pub password: Option<String>,
    pub previous_password: Option<String>,
}

pub async fn update_user(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    if !session.is_admin() && session.user_id != id {
        return Err(HomarrError::Forbidden("Forbidden".into()));
    }
    let new_password = body.password.filter(|p| !p.is_empty());
    if new_password.as_ref().is_some_and(|p| p.chars().count() < 8) {
        return Err(HomarrError::BadRequest(
            "Password must be >= 8 chars".into(),
        ));
    }
    if new_password.is_some() {
        let target = users::find_by_id(&state.db, &id)
            .await?
            .ok_or_else(|| HomarrError::NotFound("User not found".into()))?;
        if target.provider != "credentials" {
            return Err(HomarrError::Forbidden(
                "Password can only be changed for credentials users".into(),
            ));
        }
        // Changing your own password (admins too) requires the current one, as upstream.
        if session.user_id == id {
            let previous = body.previous_password.as_deref().unwrap_or_default();
            let valid = match target.password.as_deref() {
                Some(hash) if !previous.is_empty() => verify_password(previous, hash).await?,
                _ => false,
            };
            if !valid {
                return Err(HomarrError::BadRequest(
                    "Current password is missing or incorrect".into(),
                ));
            }
        }
    }
    let name = body.name.as_deref().map(str::trim);
    // Upstream ignores an empty email rather than clearing it.
    let email = body.email.as_deref().filter(|e| !e.is_empty());
    if name.is_some() || email.is_some() {
        let target = users::find_by_id(&state.db, &id)
            .await?
            .ok_or_else(|| HomarrError::NotFound("User not found".into()))?;
        let changes_identity = name.is_some_and(|n| target.name.as_deref() != Some(n))
            || email.is_some_and(|e| target.email.as_deref() != Some(e));
        if changes_identity && target.provider != "credentials" {
            return Err(HomarrError::Forbidden(
                "Username and email can not be changed for users with external providers".into(),
            ));
        }
    }
    if let Some(name) = name {
        // No unique index on "user"."name": case-insensitive duplicates make login ambiguous.
        if let Some(existing) = users::find_by_name_ci(&state.db, name).await? {
            if existing.id != id {
                return Err(HomarrError::Conflict("Username already taken".into()));
            }
        }
    }
    users::update_profile(&state.db, &id, name, email, body.image.as_deref()).await?;
    if let Some(pw) = new_password {
        let hash = hash_password(&pw).await?;
        users::update_password(&state.db, &id, &hash).await?;
    }
    let updated = users::find_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("User not found".into()))?;
    Ok(Json(json!({ "user": updated })))
}

pub async fn delete_user(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    if admin.user_id == id {
        return Err(HomarrError::BadRequest(
            "Cannot delete the currently logged-in admin".into(),
        ));
    }
    let deleted = users::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("User not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesBody {
    pub home_board_id: Option<Option<String>>,
    pub mobile_home_board_id: Option<Option<String>>,
    pub default_search_engine_id: Option<Option<String>>,
    pub open_search_in_new_tab: Option<bool>,
    pub ddg_bangs: Option<bool>,
    pub color_scheme: Option<String>,
    pub byte_unit_system: Option<String>,
    pub first_day_of_week: Option<i32>,
    pub ping_icons_enabled: Option<bool>,
    pub enable_right_click_on_widgets: Option<bool>,
    pub header_preferences: Option<String>,
    pub completed_manage_tour: Option<bool>,
    pub completed_board_tour: Option<bool>,
}

pub async fn update_my_preferences(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<PreferencesBody>,
) -> HomarrResult<Json<Value>> {
    session.require_writable(state.demo_read_only_active())?;
    for board_id in [&body.home_board_id, &body.mobile_home_board_id]
        .into_iter()
        .filter_map(|b| b.as_ref()?.as_deref())
    {
        let board = boards::get_by_id(&state.db, board_id)
            .await?
            .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?;
        if !can_view_board(&state.db, Some(&session), &board).await? {
            return Err(HomarrError::NotFound("Board not found".into()));
        }
    }
    let updated = users::update_preferences(
        &state.db,
        &session.user_id,
        UpdateUserPreferencesInput {
            home_board_id: body.home_board_id,
            mobile_home_board_id: body.mobile_home_board_id,
            default_search_engine_id: body.default_search_engine_id,
            open_search_in_new_tab: body.open_search_in_new_tab,
            ddg_bangs: body.ddg_bangs,
            color_scheme: body.color_scheme,
            byte_unit_system: body.byte_unit_system,
            first_day_of_week: body.first_day_of_week,
            ping_icons_enabled: body.ping_icons_enabled,
            enable_right_click_on_widgets: body.enable_right_click_on_widgets,
            header_preferences: body.header_preferences,
            completed_manage_tour: body.completed_manage_tour,
            completed_board_tour: body.completed_board_tour,
        },
    )
    .await?;
    Ok(Json(json!({ "user": updated })))
}

pub async fn list_groups(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let all = groups::list_all(&state.db).await?;
    let mut detailed = Vec::with_capacity(all.len());
    for g in all {
        if let Some(d) = groups::get_with_details(&state.db, &g.id).await? {
            detailed.push(d);
        }
    }
    Ok(Json(json!({ "items": detailed })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupBody {
    pub name: String,
}

pub async fn create_group(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CreateGroupBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    reject_reserved_group_name(&body.name)?;
    let all = groups::list_all(&state.db).await?;
    let pos = all.len() as i32;
    let group = groups::create(&state.db, body.name.trim(), Some(&admin.user_id), pos).await?;
    Ok((StatusCode::CREATED, Json(json!({ "group": group }))))
}

pub async fn get_group(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    let d = groups::get_with_details(&state.db, &id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Group not found".into()))?;
    Ok(Json(json!({ "group": d })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupBody {
    pub name: Option<String>,
    pub home_board_id: Option<Option<String>>,
    pub mobile_home_board_id: Option<Option<String>>,
}

pub async fn update_group(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateGroupBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    if let Some(name) = body.name.as_deref() {
        let group = groups::get_by_id(&state.db, &id)
            .await?
            .ok_or_else(|| HomarrError::NotFound("Group not found".into()))?;
        // Re-sending the unchanged name (e.g. alongside home board settings) is not a rename.
        if group.name != name.trim() {
            reject_reserved_group(&group)?;
            reject_reserved_group_name(name)?;
        }
    }
    let home = body.home_board_id;
    let mobile = body.mobile_home_board_id;
    let updated = groups::update_settings(
        &state.db,
        &id,
        body.name.as_deref().map(str::trim),
        home.as_ref().map(|o| o.as_deref()),
        mobile.as_ref().map(|o| o.as_deref()),
    )
    .await?;
    Ok(Json(json!({ "group": updated })))
}

pub async fn delete_group(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    reject_reserved_group_id(&state, &id).await?;
    let deleted = groups::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Group not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct GroupPermissionsBody {
    pub permissions: Vec<String>,
}

pub async fn set_group_permissions(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<GroupPermissionsBody>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    groups::set_permissions(&state.db, &id, &body.permissions).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn add_group_member(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path((id, user_id)): Path<(String, String)>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    reject_reserved_group_id(&state, &id).await?;
    throw_if_group_members_cannot_be_managed_locally(&state)?;
    let user = users::find_by_id(&state.db, &user_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("User not found".into()))?;
    if !is_group_membership_managed_locally(&user.provider) {
        return Err(HomarrError::Forbidden(
            "User's provider is not managed locally".into(),
        ));
    }
    groups::add_member(&state.db, &id, &user_id).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_group_member(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path((id, user_id)): Path<(String, String)>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    reject_reserved_group_id(&state, &id).await?;
    throw_if_group_members_cannot_be_managed_locally(&state)?;
    groups::remove_member(&state.db, &id, &user_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Upstream `isGroupMembershipManagedLocally`: only credentials users here, since OIDC
/// (with `AUTH_OIDC_GROUPS_LOCAL_MANAGEMENT`) and LDAP are not implemented in this port.
fn is_group_membership_managed_locally(provider: &str) -> bool {
    provider == "credentials"
}

/// Upstream `canManageGroupMembersLocally`: at least one enabled provider is managed locally.
fn throw_if_group_members_cannot_be_managed_locally(state: &AppState) -> HomarrResult<()> {
    if state
        .config
        .auth_providers
        .iter()
        .any(|p| is_group_membership_managed_locally(p))
    {
        Ok(())
    } else {
        Err(HomarrError::Forbidden(
            "Group members cannot be managed locally".into(),
        ))
    }
}

fn reject_reserved_group(group: &GroupRecord) -> HomarrResult<()> {
    if group.name == EVERYONE_GROUP {
        return Err(HomarrError::Forbidden(
            "Action is forbidden for reserved group names".into(),
        ));
    }
    Ok(())
}

fn reject_reserved_group_name(name: &str) -> HomarrResult<()> {
    if name.trim().eq_ignore_ascii_case(EVERYONE_GROUP) {
        return Err(HomarrError::Conflict("Group name is reserved".into()));
    }
    Ok(())
}

async fn reject_reserved_group_id(state: &AppState, id: &str) -> HomarrResult<()> {
    match groups::get_by_id(&state.db, id).await? {
        Some(group) => reject_reserved_group(&group),
        None => Ok(()),
    }
}

pub async fn list_invites(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
) -> HomarrResult<Json<Value>> {
    let items = invites::list_all(&state.db).await?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteBody {
    pub expires_in_hours: Option<i64>,
}

pub async fn create_invite(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Json(body): Json<CreateInviteBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    admin.require_writable(state.demo_read_only_active())?;
    let hours = body.expires_in_hours.unwrap_or(72).clamp(1, 720);
    let exp = Utc::now().timestamp() + hours * 3600;
    let invite = invites::create(&state.db, &admin.user_id, exp).await?;
    Ok((StatusCode::CREATED, Json(json!({ "invite": invite }))))
}

pub async fn delete_invite(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> HomarrResult<Json<Value>> {
    admin.require_writable(state.demo_read_only_active())?;
    let deleted = invites::delete(&state.db, &id).await?;
    if !deleted {
        return Err(HomarrError::NotFound("Invite not found".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}
