use std::collections::HashSet;

use homarr_common::{HomarrError, HomarrResult};
use homarr_db::models::BoardRecord;
use homarr_db::{repos, Database};
use homarr_definitions::{BoardPermission, GroupPermissionKey, IntegrationPermission};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub user_id: String,
    pub username: Option<String>,
    pub email: Option<String>,
    pub image: Option<String>,
    pub color_scheme: String,
    pub permissions: HashSet<GroupPermissionKey>,
    pub group_ids: Vec<String>,
    pub api_key_read_only: bool,
    pub oauth_client_id: Option<String>,
}

impl AuthSession {
    #[allow(clippy::too_many_arguments)]
    pub fn from_user(
        user_id: String,
        username: Option<String>,
        email: Option<String>,
        image: Option<String>,
        color_scheme: String,
        permissions: Vec<GroupPermissionKey>,
        group_ids: Vec<String>,
        api_key_read_only: bool,
        oauth_client_id: Option<String>,
    ) -> Self {
        Self {
            user_id,
            username,
            email,
            image,
            color_scheme,
            permissions: permissions.into_iter().collect(),
            group_ids,
            api_key_read_only,
            oauth_client_id,
        }
    }

    pub fn has_permission(&self, key: GroupPermissionKey) -> bool {
        self.permissions.contains(&key)
    }

    pub fn is_admin(&self) -> bool {
        self.has_permission(GroupPermissionKey::Admin)
    }

    pub fn require_permission(&self, key: GroupPermissionKey) -> HomarrResult<()> {
        if self.has_permission(key) {
            Ok(())
        } else {
            Err(HomarrError::Forbidden(format!(
                "Missing required permission: {}",
                key.as_str()
            )))
        }
    }

    pub fn require_writable(&self, demo_read_only_active: bool) -> HomarrResult<()> {
        if self.api_key_read_only {
            return Err(HomarrError::Forbidden(
                "This API key is read-only and cannot perform write operations".to_string(),
            ));
        }
        if demo_read_only_active {
            return Err(HomarrError::Forbidden("Demo mode is read-only".to_string()));
        }
        Ok(())
    }
}

pub async fn can_view_board(
    db: &Database,
    session: Option<&AuthSession>,
    board: &BoardRecord,
) -> HomarrResult<bool> {
    if board.is_public {
        return Ok(true);
    }
    let Some(sess) = session else {
        return Ok(false);
    };
    Ok(
        check_board_permission(db, Some(sess), board, BoardPermission::View)
            .await
            .is_ok(),
    )
}

pub async fn check_board_permission(
    db: &Database,
    session: Option<&AuthSession>,
    board: &BoardRecord,
    required: BoardPermission,
) -> HomarrResult<()> {
    if required == BoardPermission::View && board.is_public {
        return Ok(());
    }
    let sess = session.ok_or_else(|| {
        HomarrError::Unauthorized("Authentication required to access this board".to_string())
    })?;

    if sess.has_permission(required.global_equivalent()) {
        return Ok(());
    }
    if board.creator_id.as_deref() == Some(sess.user_id.as_str()) {
        return Ok(());
    }

    let (user_perms, group_perms) = repos::boards::get_permissions(db, &board.id).await?;
    for entry in user_perms {
        if entry.user_id == sess.user_id {
            if let Some(granted) = BoardPermission::from_str_opt(&entry.permission) {
                if required.implied_by(granted) {
                    return Ok(());
                }
            }
        }
    }
    for entry in group_perms {
        if sess.group_ids.contains(&entry.group_id) {
            if let Some(granted) = BoardPermission::from_str_opt(&entry.permission) {
                if required.implied_by(granted) {
                    return Ok(());
                }
            }
        }
    }

    Err(HomarrError::Forbidden(format!(
        "Missing '{}' permission for board '{}'",
        required.as_str(),
        board.name
    )))
}

pub async fn check_integration_permission(
    db: &Database,
    session: &AuthSession,
    integration_id: &str,
    required: IntegrationPermission,
) -> HomarrResult<()> {
    if session.has_permission(required.global_equivalent()) {
        return Ok(());
    }

    let (user_perms, group_perms) =
        repos::integrations::get_permissions(db, integration_id).await?;
    for entry in user_perms {
        if entry.user_id == session.user_id {
            if let Some(granted) = IntegrationPermission::from_str_opt(&entry.permission) {
                if required.implied_by(granted) {
                    return Ok(());
                }
            }
        }
    }
    for entry in group_perms {
        if session.group_ids.contains(&entry.group_id) {
            if let Some(granted) = IntegrationPermission::from_str_opt(&entry.permission) {
                if required.implied_by(granted) {
                    return Ok(());
                }
            }
        }
    }

    Err(HomarrError::Forbidden(format!(
        "Missing '{}' permission for integration {}",
        required.as_str(),
        integration_id
    )))
}
