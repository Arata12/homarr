use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ColorScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportedAuthProvider {
    #[default]
    Credentials,
    Oidc,
    Ldap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchEngineType {
    #[default]
    #[serde(rename = "generic")]
    Generic,
    #[serde(rename = "fromIntegration")]
    FromIntegration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnboardingStep {
    #[default]
    Start,
    User,
    Group,
    Setup,
    Finish,
}

impl OnboardingStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::User => "user",
            Self::Group => "group",
            Self::Setup => "setup",
            Self::Finish => "finish",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "start" => Some(Self::Start),
            "user" => Some(Self::User),
            "group" => Some(Self::Group),
            "setup" => Some(Self::Setup),
            "finish" => Some(Self::Finish),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnboardingLayoutPreset {
    #[default]
    Balanced,
    Wide,
    Focused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    #[default]
    Default,
    Absolutely,
    Ayu,
    Cappuccin,
    Dracula,
    Everforest,
    Github,
    Gruvbox,
    Linear,
    Lobster,
    Material,
    Matrix,
    Monokai,
    NightOwl,
    Nord,
    Notion,
    Og,
    One,
    Oscurange,
    Raycast,
    RosePine,
    Sentry,
    Shadcn,
    Solarized,
    Temple,
    TokyoNight,
    Vercel,
    VscodePlus,
    Xcode,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthBranding {
    pub show_app_name: bool,
    pub show_logo: bool,
    pub show_greeting: bool,
}

impl Default for AuthBranding {
    fn default() -> Self {
        Self {
            show_app_name: true,
            show_logo: true,
            show_greeting: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandingSettings {
    pub theme_id: ThemeId,
    pub custom_theme: Option<Value>,
    pub app_name: String,
    pub greeting: String,
    pub logo_image_url: Option<String>,
    pub favicon_image_url: Option<String>,
    pub sign_in_background_image_url: Option<String>,
    pub sign_in_background_overlay: f64,
    pub show_sign_in_marquee: bool,
    pub auth_branding: AuthBranding,
}

impl Default for BrandingSettings {
    fn default() -> Self {
        Self {
            theme_id: ThemeId::Default,
            custom_theme: None,
            app_name: "Homarr".to_string(),
            greeting: String::new(),
            logo_image_url: None,
            favicon_image_url: None,
            sign_in_background_image_url: None,
            sign_in_background_overlay: 0.55,
            show_sign_in_marquee: true,
            auth_branding: AuthBranding::default(),
        }
    }
}

pub fn parse_branding_settings(value: &Value) -> BrandingSettings {
    let Some(obj) = value.as_object() else {
        return BrandingSettings::default();
    };
    let defaults = BrandingSettings::default();

    let mut auth_branding = defaults.auth_branding.clone();
    if let Some(v) = obj
        .get("showCustomAppNameOnLogin")
        .and_then(|v| v.as_bool())
    {
        auth_branding.show_app_name = v;
    }
    if let Some(v) = obj.get("showCustomLogoOnLogin").and_then(|v| v.as_bool()) {
        auth_branding.show_logo = v;
    }
    if let Some(v) = obj
        .get("showCustomGreetingOnLogin")
        .and_then(|v| v.as_bool())
    {
        auth_branding.show_greeting = v;
    }
    if let Some(ab) = obj.get("authBranding").and_then(|v| v.as_object()) {
        if let Some(v) = ab.get("showAppName").and_then(|v| v.as_bool()) {
            auth_branding.show_app_name = v;
        }
        if let Some(v) = ab.get("showLogo").and_then(|v| v.as_bool()) {
            auth_branding.show_logo = v;
        }
        if let Some(v) = ab.get("showGreeting").and_then(|v| v.as_bool()) {
            auth_branding.show_greeting = v;
        }
    }

    let app_name = obj
        .get("appName")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .map(str::to_string)
        .unwrap_or(defaults.app_name);

    let greeting = obj
        .get("greeting")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.len() <= 160)
        .map(str::to_string)
        .unwrap_or(defaults.greeting);

    let theme_id: ThemeId = obj
        .get("themeId")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(ThemeId::Default);

    let custom_theme = obj.get("customTheme").cloned().filter(|v| !v.is_null());

    BrandingSettings {
        theme_id: if theme_id == ThemeId::Custom && custom_theme.is_none() {
            ThemeId::Default
        } else {
            theme_id
        },
        custom_theme,
        app_name,
        greeting,
        logo_image_url: obj
            .get("logoImageUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        favicon_image_url: obj
            .get("faviconImageUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        sign_in_background_image_url: obj
            .get("signInBackgroundImageUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        sign_in_background_overlay: obj
            .get("signInBackgroundOverlay")
            .and_then(|v| v.as_f64())
            .map(|n| n.clamp(0.0, 0.9))
            .unwrap_or(defaults.sign_in_background_overlay),
        show_sign_in_marquee: obj
            .get("showSignInMarquee")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.show_sign_in_marquee),
        auth_branding,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSettings {
    pub enable_general: bool,
    pub instance_id: Option<String>,
    pub last_successful_snapshot_at: Option<String>,
}

impl Default for AnalyticsSettings {
    fn default() -> Self {
        Self {
            enable_general: true,
            instance_id: None,
            last_successful_snapshot_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlingSettings {
    pub no_index: bool,
    pub no_follow: bool,
    pub no_translate: bool,
    pub no_site_links_search_box: bool,
}

impl Default for CrawlingSettings {
    fn default() -> Self {
        Self {
            no_index: true,
            no_follow: true,
            no_translate: true,
            no_site_links_search_box: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardServerSettings {
    pub home_board_id: Option<String>,
    pub mobile_home_board_id: Option<String>,
    pub enable_status_by_default: bool,
    pub force_disable_status: bool,
}

impl Default for BoardServerSettings {
    fn default() -> Self {
        Self {
            home_board_id: None,
            mobile_home_board_id: None,
            enable_status_by_default: true,
            force_disable_status: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserServerSettings {
    pub enable_gravatar: bool,
}

impl Default for UserServerSettings {
    fn default() -> Self {
        Self {
            enable_gravatar: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub default_color_scheme: ColorScheme,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            default_color_scheme: ColorScheme::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CultureSettings {
    pub default_locale: String,
}

impl Default for CultureSettings {
    fn default() -> Self {
        Self {
            default_locale: "en".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchServerSettings {
    pub default_search_engine_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewSettings {
    pub enabled: bool,
    pub last_seen_version: Option<String>,
}

impl Default for WhatsNewSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            last_seen_version: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    pub analytics: AnalyticsSettings,
    pub crawling_and_indexing: CrawlingSettings,
    pub board: BoardServerSettings,
    pub user: UserServerSettings,
    pub appearance: AppearanceSettings,
    pub branding: BrandingSettings,
    pub culture: CultureSettings,
    pub search: SearchServerSettings,
    pub whats_new: WhatsNewSettings,
}

pub const DEFAULT_SERVER_SETTINGS_KEYS: [&str; 9] = [
    "analytics",
    "crawlingAndIndexing",
    "board",
    "user",
    "appearance",
    "branding",
    "culture",
    "search",
    "whatsNew",
];

pub fn default_server_setting_json(key: &str) -> Option<Value> {
    let settings = ServerSettings::default();
    match key {
        "analytics" => serde_json::to_value(settings.analytics).ok(),
        "crawlingAndIndexing" => serde_json::to_value(settings.crawling_and_indexing).ok(),
        "board" => serde_json::to_value(settings.board).ok(),
        "user" => serde_json::to_value(settings.user).ok(),
        "appearance" => serde_json::to_value(settings.appearance).ok(),
        "branding" => serde_json::to_value(settings.branding).ok(),
        "culture" => serde_json::to_value(settings.culture).ok(),
        "search" => serde_json::to_value(settings.search).ok(),
        "whatsNew" => serde_json::to_value(settings.whats_new).ok(),
        _ => None,
    }
}

pub fn default_header_preferences_serialized() -> String {
    json!({
        "version": 4,
        "visible": true,
        "searchDisplay": "input",
        "logoDisplay": "logoAndText",
        "tabsDisplay": "compact",
        "zones": {
            "left": [{ "type": "builtin", "id": "logo" }],
            "center": [{ "type": "builtin", "id": "search" }],
            "right": [
                { "type": "builtin", "id": "boardTabs" },
                { "type": "builtin", "id": "boardEdit" },
                { "type": "builtin", "id": "boardSettings" },
                { "type": "builtin", "id": "user" }
            ]
        }
    })
    .to_string()
}
