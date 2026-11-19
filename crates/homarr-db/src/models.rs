use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRecord {
    pub id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<i64>,
    pub image: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub provider: String,
    pub home_board_id: Option<String>,
    pub mobile_home_board_id: Option<String>,
    pub default_search_engine_id: Option<String>,
    pub open_search_in_new_tab: bool,
    pub ddg_bangs: bool,
    pub color_scheme: String,
    pub byte_unit_system: String,
    pub first_day_of_week: i32,
    pub ping_icons_enabled: bool,
    pub enable_right_click_on_widgets: bool,
    pub header_preferences: String,
    pub completed_manage_tour: bool,
    pub completed_board_tour: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWithUser {
    pub session_token: String,
    pub user_id: String,
    pub expires_ms: i64,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub user_image: Option<String>,
    pub user_color_scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyRecord {
    pub id: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub read_only: bool,
    pub oauth_client_id: Option<String>,
    pub oauth_client_name: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_issued_at_ms: Option<i64>,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyWithUser {
    pub api_key: ApiKeyRecord,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub user_color_scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRecord {
    pub id: String,
    pub name: String,
    pub owner_id: Option<String>,
    pub home_board_id: Option<String>,
    pub mobile_home_board_id: Option<String>,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberUser {
    pub id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupWithDetails {
    pub group: GroupRecord,
    pub permissions: Vec<String>,
    pub members: Vec<GroupMemberUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteRecord {
    pub id: String,
    pub token: String,
    pub expiration_date: i64,
    pub creator_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRecord {
    pub id: String,
    pub name: String,
    pub content: Vec<u8>,
    pub content_type: String,
    pub size: i32,
    pub created_at: i64,
    pub creator_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSummary {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: i32,
    pub created_at: i64,
    pub creator_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: String,
    pub href: Option<String>,
    pub ping_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSecretRecord {
    pub kind: String,
    pub value: String,
    pub updated_at: i64,
    pub integration_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    pub external_url: Option<String>,
    pub kind: String,
    pub app_id: Option<String>,
    pub secret_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardRecord {
    pub id: String,
    pub name: String,
    pub is_public: bool,
    pub creator_id: Option<String>,
    pub page_title: Option<String>,
    pub meta_title: Option<String>,
    pub logo_image_url: Option<String>,
    pub favicon_image_url: Option<String>,
    pub background_image_url: Option<String>,
    pub background_image_attachment: String,
    pub background_image_repeat: String,
    pub background_image_size: String,
    pub primary_color: Option<String>,
    pub secondary_color: Option<String>,
    pub opacity: i32,
    pub custom_css: Option<String>,
    pub icon_color: Option<String>,
    pub item_radius: Option<String>,
    pub disable_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRecord {
    pub id: String,
    pub name: String,
    pub board_id: String,
    pub column_count: i32,
    pub left_gutter_column_count: i32,
    pub right_gutter_column_count: i32,
    pub breakpoint: i32,
    pub role: String,
    pub resize_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionRecord {
    pub id: String,
    pub board_id: String,
    pub kind: String,
    pub x_offset: Option<i32>,
    pub y_offset: Option<i32>,
    pub name: Option<String>,
    pub options: Value,
    pub layouts: Vec<SectionLayoutRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionLayoutRecord {
    pub section_id: String,
    pub layout_id: String,
    pub parent_section_id: Option<String>,
    pub x_offset: i32,
    pub y_offset: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemLayoutRecord {
    pub item_id: String,
    pub section_id: String,
    pub layout_id: String,
    pub x_offset: i32,
    pub y_offset: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRecord {
    pub id: String,
    pub board_id: String,
    pub kind: String,
    pub options: Value,
    pub advanced_options: Value,
    pub layouts: Vec<ItemLayoutRecord>,
    pub integration_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullBoard {
    #[serde(flatten)]
    pub board: BoardRecord,
    pub layouts: Vec<LayoutRecord>,
    pub sections: Vec<SectionRecord>,
    pub items: Vec<ItemRecord>,
    pub user_permissions: Vec<UserPermissionEntry>,
    pub group_permissions: Vec<GroupPermissionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPermissionEntry {
    pub user_id: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPermissionEntry {
    pub group_id: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEngineRecord {
    pub id: String,
    pub icon_url: String,
    pub name: String,
    pub short: String,
    pub description: Option<String>,
    pub url_template: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub integration_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconRepositoryRecord {
    pub id: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    pub checksum: String,
    pub icon_repository_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingRecord {
    pub id: String,
    pub step: String,
    pub previous_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomWidgetV2Record {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub sources: Value,
    pub requests: Value,
    pub options: Value,
    pub template: String,
    pub workshop: Option<Value>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub creator_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomWidgetV2SecretRecord {
    pub definition_id: String,
    pub source_id: String,
    pub kind: String,
    pub encrypted_value: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConfigRecord {
    pub id: String,
    pub enabled: bool,
    pub web_search_enabled: bool,
    pub provider: String,
    pub base_url: String,
    pub model_discovery_path: Option<String>,
    pub encrypted_api_key: Option<String>,
    pub encrypted_headers: Option<String>,
    pub model_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantThreadRecord {
    pub id: String,
    pub user_id: String,
    pub title: Option<String>,
    pub model_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageRecord {
    pub id: String,
    pub thread_id: String,
    pub parent_id: Option<String>,
    pub format: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobConfigRecord {
    pub name: String,
    pub cron_expression: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedCertificateRecord {
    pub hostname: String,
    pub thumbprint: String,
    pub certificate: String,
}
