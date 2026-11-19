pub mod board;
pub mod docker_discovery;
pub mod integration;
pub mod permissions;
pub mod section;
pub mod settings;
pub mod widget;

pub use board::{
    normalize_board_layout_roles, BackgroundImageAttachment, BackgroundImageRepeat,
    BackgroundImageSize, CornerStyle, LayoutRole,
};
pub use docker_discovery::{
    build_docker_service_url_candidates, extract_container_image_name, match_docker_service,
    match_integration_kind, match_integration_kind_from_container, normalize_docker_service_url,
    DockerServiceMatch, DockerServicePort, DockerServiceUrlCandidate,
};
pub use integration::{
    all_integration_defs, get_integration_api_key_url, get_integration_def,
    get_integration_kinds_by_category, is_http_integration_kind, IntegrationCategory,
    IntegrationDefinition, IntegrationHttpAuthDefinition, IntegrationHttpBodyAuth,
    IntegrationHttpStandardAuth, IntegrationKind, IntegrationSecretKind, IntegrationSecretMetadata,
};
pub use permissions::{
    get_app_management_access, get_integration_management_access, get_permissions_with_children,
    get_permissions_with_parents, is_capability_implied_by_level, matrix_state_to_permissions,
    permission_matrix, permissions_to_matrix_state, BoardPermission, GroupPermissionKey,
    IntegrationManagementAccess, IntegrationPermission, ManagementSectionAccess,
    PermissionMatrixCategory, PermissionMatrixCategoryState, PermissionMatrixState,
};
pub use section::{
    compare_root_sections, get_board_lane_column_count, get_board_tab_sections,
    get_root_section_lane, project_root_sections, BoardLane, LaneGeometry, RootSectionLike,
    SectionKind, MAX_BOARD_TABS,
};
pub use settings::{
    default_header_preferences_serialized, parse_branding_settings, AuthBranding, BrandingSettings,
    ColorScheme, OnboardingLayoutPreset, OnboardingStep, SearchEngineType, ServerSettings,
    SupportedAuthProvider, ThemeId,
};
pub use widget::{
    all_widget_kinds, get_widget_integration_issue, widget_default_size,
    widget_integration_support, widget_requires_integration, WidgetIntegrationIssue, WidgetKind,
};
