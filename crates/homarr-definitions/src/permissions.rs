use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardPermission {
    View,
    Modify,
    Full,
}

impl BoardPermission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Modify => "modify",
            Self::Full => "full",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "view" => Some(Self::View),
            "modify" => Some(Self::Modify),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn global_equivalent(self) -> GroupPermissionKey {
        match self {
            Self::View => GroupPermissionKey::BoardViewAll,
            Self::Modify => GroupPermissionKey::BoardModifyAll,
            Self::Full => GroupPermissionKey::BoardFullAll,
        }
    }

    pub fn implied_by(self, granted: Self) -> bool {
        match self {
            Self::View => true,
            Self::Modify => matches!(granted, Self::Modify | Self::Full),
            Self::Full => matches!(granted, Self::Full),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationPermission {
    Use,
    Interact,
    Full,
}

impl IntegrationPermission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Interact => "interact",
            Self::Full => "full",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "use" => Some(Self::Use),
            "interact" => Some(Self::Interact),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn global_equivalent(self) -> GroupPermissionKey {
        match self {
            Self::Use => GroupPermissionKey::IntegrationUseAll,
            Self::Interact => GroupPermissionKey::IntegrationInteractAll,
            Self::Full => GroupPermissionKey::IntegrationFullAll,
        }
    }

    pub fn implied_by(self, granted: Self) -> bool {
        match self {
            Self::Use => true,
            Self::Interact => matches!(granted, Self::Interact | Self::Full),
            Self::Full => matches!(granted, Self::Full),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GroupPermissionKey {
    #[serde(rename = "board-create")]
    BoardCreate,
    #[serde(rename = "board-view-all")]
    BoardViewAll,
    #[serde(rename = "board-modify-all")]
    BoardModifyAll,
    #[serde(rename = "board-full-all")]
    BoardFullAll,
    #[serde(rename = "app-create")]
    AppCreate,
    #[serde(rename = "app-use-all")]
    AppUseAll,
    #[serde(rename = "app-modify-all")]
    AppModifyAll,
    #[serde(rename = "app-full-all")]
    AppFullAll,
    #[serde(rename = "integration-create")]
    IntegrationCreate,
    #[serde(rename = "integration-use-all")]
    IntegrationUseAll,
    #[serde(rename = "integration-interact-all")]
    IntegrationInteractAll,
    #[serde(rename = "integration-full-all")]
    IntegrationFullAll,
    #[serde(rename = "search-engine-create")]
    SearchEngineCreate,
    #[serde(rename = "search-engine-modify-all")]
    SearchEngineModifyAll,
    #[serde(rename = "search-engine-full-all")]
    SearchEngineFullAll,
    #[serde(rename = "media-upload")]
    MediaUpload,
    #[serde(rename = "media-view-all")]
    MediaViewAll,
    #[serde(rename = "media-full-all")]
    MediaFullAll,
    #[serde(rename = "other-view-logs")]
    OtherViewLogs,
    #[serde(rename = "admin")]
    Admin,
}

impl GroupPermissionKey {
    pub const ALL: [Self; 20] = [
        Self::BoardCreate,
        Self::BoardViewAll,
        Self::BoardModifyAll,
        Self::BoardFullAll,
        Self::AppCreate,
        Self::AppUseAll,
        Self::AppModifyAll,
        Self::AppFullAll,
        Self::IntegrationCreate,
        Self::IntegrationUseAll,
        Self::IntegrationInteractAll,
        Self::IntegrationFullAll,
        Self::SearchEngineCreate,
        Self::SearchEngineModifyAll,
        Self::SearchEngineFullAll,
        Self::MediaUpload,
        Self::MediaViewAll,
        Self::MediaFullAll,
        Self::OtherViewLogs,
        Self::Admin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BoardCreate => "board-create",
            Self::BoardViewAll => "board-view-all",
            Self::BoardModifyAll => "board-modify-all",
            Self::BoardFullAll => "board-full-all",
            Self::AppCreate => "app-create",
            Self::AppUseAll => "app-use-all",
            Self::AppModifyAll => "app-modify-all",
            Self::AppFullAll => "app-full-all",
            Self::IntegrationCreate => "integration-create",
            Self::IntegrationUseAll => "integration-use-all",
            Self::IntegrationInteractAll => "integration-interact-all",
            Self::IntegrationFullAll => "integration-full-all",
            Self::SearchEngineCreate => "search-engine-create",
            Self::SearchEngineModifyAll => "search-engine-modify-all",
            Self::SearchEngineFullAll => "search-engine-full-all",
            Self::MediaUpload => "media-upload",
            Self::MediaViewAll => "media-view-all",
            Self::MediaFullAll => "media-full-all",
            Self::OtherViewLogs => "other-view-logs",
            Self::Admin => "admin",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

fn direct_children(key: GroupPermissionKey) -> &'static [GroupPermissionKey] {
    use GroupPermissionKey::*;
    match key {
        BoardModifyAll => &[BoardViewAll],
        BoardFullAll => &[BoardModifyAll, BoardCreate],
        AppModifyAll => &[AppCreate],
        AppFullAll => &[AppModifyAll, AppUseAll],
        IntegrationInteractAll => &[IntegrationUseAll],
        IntegrationFullAll => &[IntegrationInteractAll, IntegrationCreate],
        SearchEngineModifyAll => &[SearchEngineCreate],
        SearchEngineFullAll => &[SearchEngineModifyAll],
        MediaFullAll => &[MediaUpload, MediaViewAll],
        Admin => &[
            BoardFullAll,
            AppFullAll,
            IntegrationFullAll,
            SearchEngineFullAll,
            MediaFullAll,
            OtherViewLogs,
        ],
        _ => &[],
    }
}

fn add_children_recursive(set: &mut HashSet<GroupPermissionKey>, key: GroupPermissionKey) {
    for &child in direct_children(key) {
        add_children_recursive(set, child);
    }
    set.insert(key);
}

pub fn get_permissions_with_children(
    permissions: &[GroupPermissionKey],
) -> Vec<GroupPermissionKey> {
    let mut set = HashSet::new();
    for &p in permissions {
        add_children_recursive(&mut set, p);
    }
    GroupPermissionKey::ALL
        .into_iter()
        .filter(|k| set.contains(k))
        .collect()
}

pub fn get_permissions_with_parents(permissions: &[GroupPermissionKey]) -> Vec<GroupPermissionKey> {
    let mut set: HashSet<GroupPermissionKey> = permissions.iter().copied().collect();
    let mut changed = true;
    while changed {
        changed = false;
        for candidate in GroupPermissionKey::ALL {
            if !set.contains(&candidate)
                && direct_children(candidate).iter().any(|c| set.contains(c))
            {
                set.insert(candidate);
                changed = true;
            }
        }
    }
    GroupPermissionKey::ALL
        .into_iter()
        .filter(|k| set.contains(k))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionMatrixCategory {
    #[serde(rename = "board")]
    Board,
    #[serde(rename = "app")]
    App,
    #[serde(rename = "integration")]
    Integration,
    #[serde(rename = "search-engine")]
    SearchEngine,
    #[serde(rename = "media")]
    Media,
    #[serde(rename = "other")]
    Other,
    #[serde(rename = "admin")]
    Admin,
}

impl PermissionMatrixCategory {
    pub const ALL: [Self; 7] = [
        Self::Board,
        Self::App,
        Self::Integration,
        Self::SearchEngine,
        Self::Media,
        Self::Other,
        Self::Admin,
    ];

    pub fn levels(self) -> &'static [GroupPermissionKey] {
        use GroupPermissionKey::*;
        match self {
            Self::Board => &[BoardViewAll, BoardModifyAll, BoardFullAll],
            Self::App => &[AppModifyAll, AppFullAll],
            Self::Integration => &[
                IntegrationUseAll,
                IntegrationInteractAll,
                IntegrationFullAll,
            ],
            Self::SearchEngine => &[SearchEngineModifyAll, SearchEngineFullAll],
            Self::Media => &[MediaViewAll, MediaFullAll],
            Self::Other => &[OtherViewLogs],
            Self::Admin => &[Admin],
        }
    }

    pub fn capabilities(self) -> &'static [GroupPermissionKey] {
        use GroupPermissionKey::*;
        match self {
            Self::Board => &[BoardCreate],
            Self::App => &[AppUseAll, AppCreate],
            Self::Integration => &[IntegrationCreate],
            Self::SearchEngine => &[SearchEngineCreate],
            Self::Media => &[MediaUpload],
            Self::Other | Self::Admin => &[],
        }
    }
}

pub fn permission_matrix() -> &'static [PermissionMatrixCategory] {
    &PermissionMatrixCategory::ALL
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionMatrixCategoryState {
    pub level: usize,
    pub capabilities: Vec<GroupPermissionKey>,
}

pub type PermissionMatrixState = HashMap<PermissionMatrixCategory, PermissionMatrixCategoryState>;

pub fn is_capability_implied_by_level(
    category: PermissionMatrixCategory,
    capability: GroupPermissionKey,
    level: usize,
) -> bool {
    if level == 0 {
        return false;
    }
    let levels = category.levels();
    let slice = &levels[..level.min(levels.len())];
    get_permissions_with_children(slice).contains(&capability)
}

pub fn permissions_to_matrix_state(permissions: &[GroupPermissionKey]) -> PermissionMatrixState {
    let set: HashSet<GroupPermissionKey> = permissions.iter().copied().collect();
    let mut state = HashMap::new();
    for category in PermissionMatrixCategory::ALL {
        let mut level = 0;
        for (idx, key) in category.levels().iter().enumerate() {
            if set.contains(key) {
                level = idx + 1;
            }
        }
        let capabilities = category
            .capabilities()
            .iter()
            .copied()
            .filter(|cap| {
                set.contains(cap) || is_capability_implied_by_level(category, *cap, level)
            })
            .collect();
        state.insert(
            category,
            PermissionMatrixCategoryState {
                level,
                capabilities,
            },
        );
    }
    state
}

pub fn matrix_state_to_permissions(state: &PermissionMatrixState) -> Vec<GroupPermissionKey> {
    let mut result = Vec::new();
    for category in PermissionMatrixCategory::ALL {
        if let Some(cat_state) = state.get(&category) {
            let levels = category.levels();
            let count = cat_state.level.min(levels.len());
            result.extend_from_slice(&levels[..count]);
            for &cap in &cat_state.capabilities {
                if !is_capability_implied_by_level(category, cap, count) {
                    result.push(cap);
                }
            }
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagementSectionAccess {
    pub can_manage_all: bool,
    pub can_create: bool,
    pub can_access: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationManagementAccess {
    pub can_manage_all: bool,
    pub can_create: bool,
    pub has_delegated_full_access: bool,
    pub can_access: bool,
}

pub fn get_app_management_access(permissions: &[GroupPermissionKey]) -> ManagementSectionAccess {
    let can_manage_all = permissions.contains(&GroupPermissionKey::AppModifyAll);
    let can_create = permissions.contains(&GroupPermissionKey::AppCreate);
    ManagementSectionAccess {
        can_manage_all,
        can_create,
        can_access: can_manage_all || can_create,
    }
}

pub fn get_integration_management_access(
    permissions: &[GroupPermissionKey],
    has_delegated_full_access: bool,
) -> IntegrationManagementAccess {
    let can_manage_all = permissions.contains(&GroupPermissionKey::IntegrationFullAll);
    let can_create = permissions.contains(&GroupPermissionKey::IntegrationCreate);
    IntegrationManagementAccess {
        can_manage_all,
        can_create,
        has_delegated_full_access,
        can_access: can_manage_all || can_create || has_delegated_full_access,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_implies_all_child_permissions() {
        let expanded = get_permissions_with_children(&[GroupPermissionKey::Admin]);
        assert_eq!(expanded.len(), GroupPermissionKey::ALL.len());
        assert!(expanded.contains(&GroupPermissionKey::BoardFullAll));
        assert!(expanded.contains(&GroupPermissionKey::OtherViewLogs));
    }

    #[test]
    fn matrix_round_trip_preserves_state_and_skips_implied_capabilities() {
        let initial = vec![
            GroupPermissionKey::BoardFullAll,
            GroupPermissionKey::BoardCreate,
            GroupPermissionKey::AppUseAll,
            GroupPermissionKey::MediaUpload,
        ];
        let state = permissions_to_matrix_state(&initial);
        assert!(is_capability_implied_by_level(
            PermissionMatrixCategory::Board,
            GroupPermissionKey::BoardCreate,
            3
        ));
        let round_tripped = matrix_state_to_permissions(&state);
        assert_eq!(
            round_tripped,
            vec![
                GroupPermissionKey::BoardViewAll,
                GroupPermissionKey::BoardModifyAll,
                GroupPermissionKey::BoardFullAll,
                GroupPermissionKey::AppUseAll,
                GroupPermissionKey::MediaUpload,
            ]
        );
        assert_eq!(permissions_to_matrix_state(&round_tripped), state);
    }
}
