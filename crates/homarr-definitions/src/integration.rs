use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegrationSecretKind {
    #[serde(rename = "apiKey")]
    ApiKey,
    #[serde(rename = "username")]
    Username,
    #[serde(rename = "password")]
    Password,
    #[serde(rename = "tokenId")]
    TokenId,
    #[serde(rename = "realm")]
    Realm,
    #[serde(rename = "personalAccessToken")]
    PersonalAccessToken,
    #[serde(rename = "topic")]
    Topic,
    #[serde(rename = "opnsenseApiKey")]
    OpnsenseApiKey,
    #[serde(rename = "opnsenseApiSecret")]
    OpnsenseApiSecret,
    #[serde(rename = "patchmonApiKey")]
    PatchmonApiKey,
    #[serde(rename = "patchmonApiSecret")]
    PatchmonApiSecret,
    #[serde(rename = "url")]
    Url,
    #[serde(rename = "privateKey")]
    PrivateKey,
    #[serde(rename = "githubAppId")]
    GithubAppId,
    #[serde(rename = "githubInstallationId")]
    GithubInstallationId,
    #[serde(rename = "slug")]
    Slug,
    #[serde(rename = "wazuhIndexerUrl")]
    WazuhIndexerUrl,
    #[serde(rename = "wazuhIndexerUsername")]
    WazuhIndexerUsername,
    #[serde(rename = "wazuhIndexerPassword")]
    WazuhIndexerPassword,
    #[serde(rename = "totpSecret")]
    TotpSecret,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSecretMetadata {
    pub is_public: bool,
    pub multiline: bool,
}

impl IntegrationSecretKind {
    pub const ALL: [Self; 20] = [
        Self::ApiKey,
        Self::Username,
        Self::Password,
        Self::TokenId,
        Self::Realm,
        Self::PersonalAccessToken,
        Self::Topic,
        Self::OpnsenseApiKey,
        Self::OpnsenseApiSecret,
        Self::PatchmonApiKey,
        Self::PatchmonApiSecret,
        Self::Url,
        Self::PrivateKey,
        Self::GithubAppId,
        Self::GithubInstallationId,
        Self::Slug,
        Self::WazuhIndexerUrl,
        Self::WazuhIndexerUsername,
        Self::WazuhIndexerPassword,
        Self::TotpSecret,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "apiKey",
            Self::Username => "username",
            Self::Password => "password",
            Self::TokenId => "tokenId",
            Self::Realm => "realm",
            Self::PersonalAccessToken => "personalAccessToken",
            Self::Topic => "topic",
            Self::OpnsenseApiKey => "opnsenseApiKey",
            Self::OpnsenseApiSecret => "opnsenseApiSecret",
            Self::PatchmonApiKey => "patchmonApiKey",
            Self::PatchmonApiSecret => "patchmonApiSecret",
            Self::Url => "url",
            Self::PrivateKey => "privateKey",
            Self::GithubAppId => "githubAppId",
            Self::GithubInstallationId => "githubInstallationId",
            Self::Slug => "slug",
            Self::WazuhIndexerUrl => "wazuhIndexerUrl",
            Self::WazuhIndexerUsername => "wazuhIndexerUsername",
            Self::WazuhIndexerPassword => "wazuhIndexerPassword",
            Self::TotpSecret => "totpSecret",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn metadata(self) -> IntegrationSecretMetadata {
        match self {
            Self::Username
            | Self::TokenId
            | Self::Realm
            | Self::Topic
            | Self::PatchmonApiKey
            | Self::GithubAppId
            | Self::GithubInstallationId
            | Self::Slug
            | Self::WazuhIndexerUrl
            | Self::WazuhIndexerUsername => IntegrationSecretMetadata {
                is_public: true,
                multiline: false,
            },
            Self::PrivateKey => IntegrationSecretMetadata {
                is_public: false,
                multiline: true,
            },
            _ => IntegrationSecretMetadata {
                is_public: false,
                multiline: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegrationCategory {
    #[serde(rename = "dnsHole")]
    DnsHole,
    #[serde(rename = "mediaService")]
    MediaService,
    #[serde(rename = "calendar")]
    Calendar,
    #[serde(rename = "mediaOrganizer")]
    MediaOrganizer,
    #[serde(rename = "mediaSearch")]
    MediaSearch,
    #[serde(rename = "mediaRelease")]
    MediaRelease,
    #[serde(rename = "mediaRequest")]
    MediaRequest,
    #[serde(rename = "downloadClient")]
    DownloadClient,
    #[serde(rename = "usenet")]
    Usenet,
    #[serde(rename = "torrent")]
    Torrent,
    #[serde(rename = "miscellaneous")]
    Miscellaneous,
    #[serde(rename = "smartHomeServer")]
    SmartHomeServer,
    #[serde(rename = "indexerManager")]
    IndexerManager,
    #[serde(rename = "healthMonitoring")]
    HealthMonitoring,
    #[serde(rename = "beszel")]
    Beszel,
    #[serde(rename = "search")]
    Search,
    #[serde(rename = "mediaTranscoding")]
    MediaTranscoding,
    #[serde(rename = "networkController")]
    NetworkController,
    #[serde(rename = "notifications")]
    Notifications,
    #[serde(rename = "firewall")]
    Firewall,
    #[serde(rename = "timetable")]
    Timetable,
    #[serde(rename = "photoService")]
    PhotoService,
    #[serde(rename = "notes")]
    Notes,
    #[serde(rename = "mediaMonitoring")]
    MediaMonitoring,
    #[serde(rename = "speedtest")]
    Speedtest,
    #[serde(rename = "analytics")]
    Analytics,
    #[serde(rename = "vpn")]
    Vpn,
    #[serde(rename = "archiving")]
    Archiving,
    #[serde(rename = "ups")]
    Ups,
    #[serde(rename = "documents")]
    Documents,
    #[serde(rename = "mediaLibrary")]
    MediaLibrary,
    #[serde(rename = "uptimeMonitoring")]
    UptimeMonitoring,
    #[serde(rename = "subtitleManager")]
    SubtitleManager,
    #[serde(rename = "reverseProxy")]
    ReverseProxy,
    #[serde(rename = "systemMonitoring")]
    SystemMonitoring,
    #[serde(rename = "security")]
    Security,
    #[serde(rename = "serviceVersion")]
    ServiceVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IntegrationHttpStandardAuth {
    None,
    Bearer {
        secret_kind: Option<IntegrationSecretKind>,
    },
    ApiKeyHeader {
        name: &'static str,
        prefix: Option<&'static str>,
        secret_kind: Option<IntegrationSecretKind>,
    },
    ApiKeyQuery {
        name: &'static str,
        secret_kind: Option<IntegrationSecretKind>,
    },
    Basic {
        username_kind: Option<IntegrationSecretKind>,
        username: Option<&'static str>,
        password_kind: Option<IntegrationSecretKind>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthModeSpec {
    pub when: &'static [IntegrationSecretKind],
    pub auth: IntegrationHttpStandardAuth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IntegrationHttpAuthDefinition {
    None,
    Bearer {
        secret_kind: Option<IntegrationSecretKind>,
    },
    ApiKeyHeader {
        name: &'static str,
        prefix: Option<&'static str>,
        secret_kind: Option<IntegrationSecretKind>,
    },
    ApiKeyQuery {
        name: &'static str,
        secret_kind: Option<IntegrationSecretKind>,
    },
    Basic {
        username_kind: Option<IntegrationSecretKind>,
        username: Option<&'static str>,
        password_kind: Option<IntegrationSecretKind>,
    },
    Modes {
        modes: &'static [AuthModeSpec],
    },
    Adapter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IntegrationHttpBodyAuth {
    JsonField { name: String, value: String },
    JsonArrayPrefix { name: String, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntegrationKind {
    #[serde(rename = "autobrr")]
    Autobrr,
    #[serde(rename = "jellystat")]
    Jellystat,
    #[serde(rename = "scrutiny")]
    Scrutiny,
    #[serde(rename = "tubearchivist")]
    Tubearchivist,
    #[serde(rename = "frigate")]
    Frigate,
    #[serde(rename = "komga")]
    Komga,
    #[serde(rename = "netalertx")]
    Netalertx,
    #[serde(rename = "jackett")]
    Jackett,
    #[serde(rename = "yourSpotify")]
    YourSpotify,
    #[serde(rename = "romm")]
    Romm,
    #[serde(rename = "homebox")]
    Homebox,
    #[serde(rename = "mealie")]
    Mealie,
    #[serde(rename = "xteve")]
    Xteve,
    #[serde(rename = "unmanic")]
    Unmanic,
    #[serde(rename = "syncthingRelay")]
    SyncthingRelay,
    #[serde(rename = "stash")]
    Stash,
    #[serde(rename = "prometheus")]
    Prometheus,
    #[serde(rename = "plantit")]
    Plantit,
    #[serde(rename = "yuvomi")]
    Yuvomi,
    #[serde(rename = "netdata")]
    Netdata,
    #[serde(rename = "fileflows")]
    Fileflows,
    #[serde(rename = "trilium")]
    Trilium,
    #[serde(rename = "tandoor")]
    Tandoor,
    #[serde(rename = "spoolman")]
    Spoolman,
    #[serde(rename = "miniflux")]
    Miniflux,
    #[serde(rename = "maintainerr")]
    Maintainerr,
    #[serde(rename = "linkwarden")]
    Linkwarden,
    #[serde(rename = "karakeep")]
    Karakeep,
    #[serde(rename = "healthchecks")]
    Healthchecks,
    #[serde(rename = "gatus")]
    Gatus,
    #[serde(rename = "changedetection")]
    Changedetection,
    #[serde(rename = "caddy")]
    Caddy,
    #[serde(rename = "sabNzbd")]
    SabNzbd,
    #[serde(rename = "nzbGet")]
    NzbGet,
    #[serde(rename = "deluge")]
    Deluge,
    #[serde(rename = "transmission")]
    Transmission,
    #[serde(rename = "qBittorrent")]
    QBittorrent,
    #[serde(rename = "aria2")]
    Aria2,
    #[serde(rename = "slskd")]
    Slskd,
    #[serde(rename = "sonarr")]
    Sonarr,
    #[serde(rename = "sportarr")]
    Sportarr,
    #[serde(rename = "radarr")]
    Radarr,
    #[serde(rename = "lidarr")]
    Lidarr,
    #[serde(rename = "readarr")]
    Readarr,
    #[serde(rename = "bindery")]
    Bindery,
    #[serde(rename = "prowlarr")]
    Prowlarr,
    #[serde(rename = "bazarr")]
    Bazarr,
    #[serde(rename = "jellyfin")]
    Jellyfin,
    #[serde(rename = "emby")]
    Emby,
    #[serde(rename = "plex")]
    Plex,
    #[serde(rename = "jellyseerr")]
    Jellyseerr,
    #[serde(rename = "seerr")]
    Seerr,
    #[serde(rename = "overseerr")]
    Overseerr,
    #[serde(rename = "piHole")]
    PiHole,
    #[serde(rename = "adGuardHome")]
    AdGuardHome,
    #[serde(rename = "technitiumDns")]
    TechnitiumDns,
    #[serde(rename = "homeAssistant")]
    HomeAssistant,
    #[serde(rename = "openmediavault")]
    Openmediavault,
    #[serde(rename = "dashDot")]
    DashDot,
    #[serde(rename = "glances")]
    Glances,
    #[serde(rename = "tdarr")]
    Tdarr,
    #[serde(rename = "proxmox")]
    Proxmox,
    #[serde(rename = "nextcloud")]
    Nextcloud,
    #[serde(rename = "unifiController")]
    UnifiController,
    #[serde(rename = "opnsense")]
    Opnsense,
    #[serde(rename = "ntfy")]
    Ntfy,
    #[serde(rename = "gotify")]
    Gotify,
    #[serde(rename = "ical")]
    Ical,
    #[serde(rename = "anchor")]
    Anchor,
    #[serde(rename = "truenas")]
    Truenas,
    #[serde(rename = "synology")]
    Synology,
    #[serde(rename = "unraid")]
    Unraid,
    #[serde(rename = "coolify")]
    Coolify,
    #[serde(rename = "immich")]
    Immich,
    #[serde(rename = "paperlessNgx")]
    PaperlessNgx,
    #[serde(rename = "thingport")]
    Thingport,
    #[serde(rename = "patchmon")]
    Patchmon,
    #[serde(rename = "tracearr")]
    Tracearr,
    #[serde(rename = "speedtestTracker")]
    SpeedtestTracker,
    #[serde(rename = "uptimeKuma")]
    UptimeKuma,
    #[serde(rename = "audiobookshelf")]
    Audiobookshelf,
    #[serde(rename = "navidrome")]
    Navidrome,
    #[serde(rename = "umami")]
    Umami,
    #[serde(rename = "peaNut")]
    PeaNut,
    #[serde(rename = "beszel")]
    Beszel,
    #[serde(rename = "gluetun")]
    Gluetun,
    #[serde(rename = "traefik")]
    Traefik,
    #[serde(rename = "archiveTeamWarrior")]
    ArchiveTeamWarrior,
    #[serde(rename = "wazuh")]
    Wazuh,
    #[serde(rename = "wud")]
    Wud,
    #[serde(rename = "llamacpp")]
    Llamacpp,
    #[serde(rename = "mock")]
    Mock,
}

impl IntegrationKind {
    pub fn as_str(self) -> &'static str {
        get_integration_def(self).kind_str
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        all_integration_defs()
            .iter()
            .find(|d| d.kind_str == s)
            .map(|d| d.kind)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationDefinition {
    pub kind: IntegrationKind,
    #[serde(rename = "kindKey")]
    pub kind_str: &'static str,
    pub name: &'static str,
    pub icon_url: &'static str,
    pub secret_kinds: &'static [&'static [IntegrationSecretKind]],
    pub optional_secret_kinds: &'static [IntegrationSecretKind],
    pub categories: &'static [IntegrationCategory],
    pub documentation_slug: Option<&'static str>,
    pub default_url: Option<&'static str>,
    pub default_port: Option<u16>,
    pub api_key_settings_path: Option<&'static str>,
    pub docker_aliases: &'static [&'static str],
    pub docker_discoverable: bool,
    pub onboarding_featured_order: Option<u8>,
    pub onboarding_hidden: bool,
    pub http_auth: IntegrationHttpAuthDefinition,
}

#[allow(clippy::too_many_arguments)]
fn def(
    kind: IntegrationKind,
    kind_str: &'static str,
    name: &'static str,
    icon_url: &'static str,
    secret_kinds: &'static [&'static [IntegrationSecretKind]],
    categories: &'static [IntegrationCategory],
    documentation_slug: Option<&'static str>,
    default_port: Option<u16>,
    http_auth: IntegrationHttpAuthDefinition,
) -> IntegrationDefinition {
    IntegrationDefinition {
        kind,
        kind_str,
        name,
        icon_url,
        secret_kinds,
        optional_secret_kinds: &[],
        categories,
        documentation_slug,
        default_url: None,
        default_port,
        api_key_settings_path: None,
        docker_aliases: &[],
        docker_discoverable: true,
        onboarding_featured_order: None,
        onboarding_hidden: false,
        http_auth,
    }
}

static DEFS: OnceLock<Vec<IntegrationDefinition>> = OnceLock::new();

pub fn all_integration_defs() -> &'static [IntegrationDefinition] {
    DEFS.get_or_init(build_all_defs)
}

pub fn get_integration_def(kind: IntegrationKind) -> &'static IntegrationDefinition {
    all_integration_defs()
        .iter()
        .find(|d| d.kind == kind)
        .expect("every IntegrationKind has a definition")
}

pub fn get_integration_kinds_by_category(cat: IntegrationCategory) -> Vec<IntegrationKind> {
    all_integration_defs()
        .iter()
        .filter(|d| d.categories.contains(&cat))
        .map(|d| d.kind)
        .collect()
}

pub fn is_http_integration_kind(kind: IntegrationKind) -> bool {
    !matches!(kind, IntegrationKind::Ical | IntegrationKind::Truenas)
}

pub fn get_integration_api_key_url(integration_url: &str, kind: IntegrationKind) -> Option<String> {
    let settings_path = get_integration_def(kind).api_key_settings_path?;
    let trimmed = integration_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let mut base = Url::parse(&normalized).ok()?;
    if base.scheme() != "http" && base.scheme() != "https" {
        return None;
    }
    let settings = Url::parse(&format!("http://localhost{settings_path}")).ok()?;
    let joined_path = format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        settings.path().trim_start_matches('/')
    );
    base.set_path(&joined_path);
    base.set_query(settings.query());
    base.set_fragment(settings.fragment());
    Some(base.to_string())
}

fn build_all_defs() -> Vec<IntegrationDefinition> {
    use IntegrationCategory as C;
    use IntegrationHttpAuthDefinition as H;
    use IntegrationHttpStandardAuth as S;
    use IntegrationKind as K;
    use IntegrationSecretKind as SK;

    const NONE: &[&[SK]] = &[&[]];
    const API_KEY: &[&[SK]] = &[&[SK::ApiKey]];
    const USER_PASS: &[&[SK]] = &[&[SK::Username, SK::Password]];
    const NONE_OR_USER_PASS: &[&[SK]] = &[&[], &[SK::Username, SK::Password]];
    const USER_PASS_OR_API_KEY: &[&[SK]] = &[&[SK::Username, SK::Password], &[SK::ApiKey]];
    const API_KEY_OR_USER_PASS: &[&[SK]] = &[&[SK::ApiKey], &[SK::Username, SK::Password]];
    const NONE_OR_API_KEY: &[&[SK]] = &[&[], &[SK::ApiKey]];

    let mut d = Vec::with_capacity(92);

    d.push(def(
        K::Autobrr,
        "autobrr",
        "Autobrr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/autobrr.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("autobrr"),
        Some(7474),
        H::ApiKeyQuery {
            name: "apikey",
            secret_kind: None,
        },
    ));
    d.push(def(
        K::Jellystat,
        "jellystat",
        "Jellystat",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/jellystat.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("jellystat"),
        Some(3000),
        H::Adapter,
    ));
    d.push(def(
        K::Scrutiny,
        "scrutiny",
        "Scrutiny",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/scrutiny.svg",
        NONE,
        &[C::Miscellaneous],
        Some("scrutiny"),
        Some(8080),
        H::Adapter,
    ));
    d.push(def(
        K::Tubearchivist,
        "tubearchivist",
        "Tube Archivist",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/tube-archivist.png",
        API_KEY,
        &[C::Miscellaneous],
        Some("tubearchivist"),
        Some(8000),
        H::Adapter,
    ));
    d.push(def(
        K::Frigate,
        "frigate",
        "Frigate",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/frigate.svg",
        USER_PASS,
        &[C::Miscellaneous],
        Some("frigate"),
        Some(8971),
        H::Adapter,
    ));
    d.push(def(
        K::Komga,
        "komga",
        "Komga",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/komga.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("komga"),
        Some(25600),
        H::ApiKeyHeader {
            name: "X-API-Key",
            prefix: None,
            secret_kind: None,
        },
    ));
    d.push(def(
        K::Netalertx,
        "netalertx",
        "NetAlertX",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/netalertx.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("netalertx"),
        Some(20212),
        H::Adapter,
    ));
    d.push(def(
        K::Jackett,
        "jackett",
        "Jackett",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/jackett.svg",
        API_KEY,
        &[C::IndexerManager],
        Some("jackett"),
        Some(9117),
        H::ApiKeyQuery {
            name: "apikey",
            secret_kind: None,
        },
    ));
    d.push(def(
        K::YourSpotify,
        "yourSpotify",
        "Your Spotify",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/your-spotify.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("your-spotify"),
        Some(8080),
        H::ApiKeyQuery {
            name: "token",
            secret_kind: None,
        },
    ));
    d.push(def(
        K::Romm,
        "romm",
        "RomM",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/romm.svg",
        NONE,
        &[C::Miscellaneous],
        Some("romm"),
        Some(8080),
        H::Adapter,
    ));
    d.push(def(
        K::Homebox,
        "homebox",
        "Homebox",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/homebox.svg",
        USER_PASS_OR_API_KEY,
        &[C::Miscellaneous],
        Some("homebox"),
        Some(7745),
        H::Adapter,
    ));
    d.push(def(
        K::Mealie,
        "mealie",
        "Mealie",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/mealie.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("mealie"),
        Some(9000),
        H::Adapter,
    ));
    d.push(def(
        K::Xteve,
        "xteve",
        "xTeVe",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/xteve.png",
        NONE_OR_USER_PASS,
        &[C::Miscellaneous],
        Some("xteve"),
        Some(34400),
        H::Adapter,
    ));
    d.push(def(
        K::Unmanic,
        "unmanic",
        "Unmanic",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/unmanic.png",
        NONE,
        &[C::Miscellaneous],
        Some("unmanic"),
        Some(8888),
        H::Adapter,
    ));
    d.push(def(
        K::SyncthingRelay,
        "syncthingRelay",
        "Syncthing Relay",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/syncthing.svg",
        NONE,
        &[C::Miscellaneous],
        Some("syncthing-relay"),
        Some(22070),
        H::Adapter,
    ));
    d.push(def(
        K::Stash,
        "stash",
        "Stash",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/stash.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("stash"),
        Some(9999),
        H::Adapter,
    ));
    d.push(def(
        K::Prometheus,
        "prometheus",
        "Prometheus",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/prometheus.svg",
        NONE_OR_USER_PASS,
        &[C::Miscellaneous],
        Some("prometheus"),
        Some(9090),
        H::Adapter,
    ));
    d.push(def(
        K::Plantit,
        "plantit",
        "Plant-it",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/plant-it.png",
        USER_PASS,
        &[C::Miscellaneous],
        Some("plantit"),
        Some(8080),
        H::Adapter,
    ));
    let mut yuvomi = def(
        K::Yuvomi,
        "yuvomi",
        "Yuvomi",
        "https://cdn.jsdelivr.net/gh/selfhst/icons/svg/yuvomi.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("yuvomi"),
        Some(3000),
        H::Bearer { secret_kind: None },
    );
    yuvomi.api_key_settings_path = Some("/settings/admin/api");
    d.push(yuvomi);
    d.push(def(
        K::Netdata,
        "netdata",
        "Netdata",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/netdata.svg",
        NONE,
        &[C::Miscellaneous],
        Some("netdata"),
        Some(19999),
        H::Adapter,
    ));
    d.push(def(
        K::Fileflows,
        "fileflows",
        "FileFlows",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/fileflows.svg",
        NONE,
        &[C::Miscellaneous],
        Some("fileflows"),
        Some(19200),
        H::Adapter,
    ));
    d.push(def(
        K::Trilium,
        "trilium",
        "Trilium",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/trilium.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("trilium"),
        Some(8080),
        H::ApiKeyHeader {
            name: "Authorization",
            prefix: None,
            secret_kind: None,
        },
    ));
    d.push(def(
        K::Tandoor,
        "tandoor",
        "Tandoor",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/tandoor-recipes.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("tandoor"),
        Some(8080),
        H::Adapter,
    ));
    d.push(def(
        K::Spoolman,
        "spoolman",
        "Spoolman",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/spoolman.svg",
        NONE,
        &[C::Miscellaneous],
        Some("spoolman"),
        Some(7912),
        H::Adapter,
    ));
    d.push(def(
        K::Miniflux,
        "miniflux",
        "Miniflux",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/miniflux.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("miniflux"),
        Some(8080),
        H::Adapter,
    ));
    d.push(def(
        K::Maintainerr,
        "maintainerr",
        "Maintainerr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/maintainerr.svg",
        NONE,
        &[C::Miscellaneous],
        Some("maintainerr"),
        Some(6246),
        H::Adapter,
    ));
    d.push(def(
        K::Linkwarden,
        "linkwarden",
        "Linkwarden",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/linkwarden.png",
        API_KEY,
        &[C::Miscellaneous],
        Some("linkwarden"),
        Some(3000),
        H::Bearer { secret_kind: None },
    ));
    d.push(def(
        K::Karakeep,
        "karakeep",
        "Karakeep",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/karakeep.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("karakeep"),
        Some(3000),
        H::Adapter,
    ));
    d.push(def(
        K::Healthchecks,
        "healthchecks",
        "Healthchecks",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/healthchecks.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("healthchecks"),
        Some(8000),
        H::Adapter,
    ));
    d.push(def(
        K::Gatus,
        "gatus",
        "Gatus",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/gatus.svg",
        NONE,
        &[C::Miscellaneous],
        Some("gatus"),
        Some(8080),
        H::Adapter,
    ));
    d.push(def(
        K::Changedetection,
        "changedetection",
        "Changedetection.io",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/changedetection.svg",
        API_KEY,
        &[C::Miscellaneous],
        Some("changedetection"),
        Some(5000),
        H::Adapter,
    ));
    d.push(def(
        K::Caddy,
        "caddy",
        "Caddy",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/caddy.svg",
        NONE,
        &[C::Miscellaneous],
        Some("caddy"),
        Some(2019),
        H::Adapter,
    ));

    let mut sab = def(
        K::SabNzbd,
        "sabNzbd",
        "SABnzbd",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/sabnzbd.svg",
        API_KEY,
        &[C::DownloadClient, C::Usenet],
        Some("sabnzbd"),
        Some(8080),
        H::ApiKeyQuery {
            name: "apikey",
            secret_kind: None,
        },
    );
    sab.api_key_settings_path = Some("/config/general/");
    sab.docker_aliases = &["sabnzbd"];
    sab.onboarding_featured_order = Some(3);
    d.push(sab);

    let mut nzb = def(
        K::NzbGet,
        "nzbGet",
        "NZBGet",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/nzbget.svg",
        USER_PASS,
        &[C::DownloadClient, C::Usenet],
        Some("nzbget"),
        Some(6789),
        H::Basic {
            username_kind: None,
            username: None,
            password_kind: None,
        },
    );
    nzb.docker_aliases = &["nzbget"];
    d.push(nzb);

    d.push(def(
        K::Deluge,
        "deluge",
        "Deluge",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/deluge.svg",
        &[&[SK::Password]],
        &[C::DownloadClient, C::Torrent],
        Some("deluge"),
        Some(8112),
        H::Adapter,
    ));
    d.push(def(
        K::Transmission,
        "transmission",
        "Transmission",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/transmission.svg",
        USER_PASS,
        &[C::DownloadClient, C::Torrent],
        Some("transmission"),
        Some(9091),
        H::Basic {
            username_kind: None,
            username: None,
            password_kind: None,
        },
    ));

    static QBIT_MODES: &[AuthModeSpec] = &[AuthModeSpec {
        when: &[SK::ApiKey],
        auth: S::Bearer { secret_kind: None },
    }];
    let mut qbit = def(
        K::QBittorrent,
        "qBittorrent",
        "qBittorrent",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/qbittorrent.svg",
        API_KEY_OR_USER_PASS,
        &[C::DownloadClient, C::Torrent],
        Some("q-bittorent"),
        Some(8080),
        H::Modes { modes: QBIT_MODES },
    );
    qbit.api_key_settings_path = Some("/#/settings/webui");
    qbit.docker_aliases = &["qbittorrent"];
    qbit.onboarding_featured_order = Some(4);
    d.push(qbit);

    d.push(def(
        K::Aria2, "aria2", "Aria2",
        "https://cdn.jsdelivr.net/gh/PapirusDevelopmentTeam/papirus_icons@latest/src/system_downloads_3.svg",
        NONE_OR_API_KEY, &[C::DownloadClient, C::Torrent, C::Miscellaneous], Some("aria2"), Some(6800), H::Adapter,
    ));
    d.push(def(
        K::Slskd,
        "slskd",
        "Slskd",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/slskd.svg",
        API_KEY,
        &[C::DownloadClient, C::Miscellaneous],
        Some("slskd"),
        Some(5030),
        H::Adapter,
    ));

    let mut sonarr = def(
        K::Sonarr,
        "sonarr",
        "Sonarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/sonarr.svg",
        API_KEY,
        &[C::Calendar, C::MediaOrganizer, C::ServiceVersion],
        Some("sonarr"),
        Some(8989),
        H::Adapter,
    );
    sonarr.api_key_settings_path = Some("/settings/general");
    sonarr.onboarding_featured_order = Some(0);
    d.push(sonarr);

    let mut sportarr = def(
        K::Sportarr,
        "sportarr",
        "Sportarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/sportarr.svg",
        API_KEY,
        &[C::Calendar, C::MediaOrganizer],
        Some("sportarr"),
        Some(1867),
        H::Adapter,
    );
    sportarr.api_key_settings_path = Some("/settings/general");
    d.push(sportarr);

    let mut radarr = def(
        K::Radarr,
        "radarr",
        "Radarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/radarr.svg",
        API_KEY,
        &[C::Calendar, C::MediaOrganizer, C::ServiceVersion],
        Some("radarr"),
        Some(7878),
        H::Adapter,
    );
    radarr.api_key_settings_path = Some("/settings/general");
    radarr.onboarding_featured_order = Some(1);
    d.push(radarr);

    let mut lidarr = def(
        K::Lidarr,
        "lidarr",
        "Lidarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/lidarr.svg",
        API_KEY,
        &[C::Calendar, C::ServiceVersion],
        Some("lidarr"),
        Some(8686),
        H::Adapter,
    );
    lidarr.api_key_settings_path = Some("/settings/general");
    d.push(lidarr);

    let mut readarr = def(
        K::Readarr,
        "readarr",
        "Readarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@main/png/readarr.png",
        API_KEY,
        &[C::Calendar, C::ServiceVersion],
        Some("readarr"),
        Some(8787),
        H::Adapter,
    );
    readarr.api_key_settings_path = Some("/settings/general");
    d.push(readarr);

    let mut bindery = def(
        K::Bindery,
        "bindery",
        "Bindery",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/bindery.svg",
        API_KEY,
        &[C::MediaOrganizer, C::ServiceVersion],
        Some("bindery"),
        Some(8787),
        H::ApiKeyHeader {
            name: "X-Api-Key",
            prefix: None,
            secret_kind: None,
        },
    );
    bindery.api_key_settings_path = Some("/settings?tab=general");
    d.push(bindery);

    let mut prowlarr = def(
        K::Prowlarr,
        "prowlarr",
        "Prowlarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/prowlarr.svg",
        API_KEY,
        &[C::IndexerManager, C::ServiceVersion],
        Some("prowlarr"),
        Some(9696),
        H::Adapter,
    );
    prowlarr.api_key_settings_path = Some("/settings/general");
    prowlarr.onboarding_featured_order = Some(2);
    d.push(prowlarr);

    let mut bazarr = def(
        K::Bazarr,
        "bazarr",
        "Bazarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/bazarr.svg",
        API_KEY,
        &[C::SubtitleManager],
        Some("bazarr"),
        Some(6767),
        H::ApiKeyHeader {
            name: "X-API-KEY",
            prefix: None,
            secret_kind: None,
        },
    );
    bazarr.api_key_settings_path = Some("/settings/general");
    bazarr.docker_aliases = &["bazarr"];
    d.push(bazarr);

    let mut jellyfin = def(
        K::Jellyfin,
        "jellyfin",
        "Jellyfin",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/jellyfin.svg",
        USER_PASS_OR_API_KEY,
        &[C::MediaService, C::MediaRelease],
        Some("jellyfin"),
        Some(8096),
        H::Adapter,
    );
    jellyfin.api_key_settings_path = Some("/web/index.html#!/dashboard/keys");
    jellyfin.onboarding_featured_order = Some(6);
    d.push(jellyfin);

    d.push(def(
        K::Emby,
        "emby",
        "Emby",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/emby.svg",
        API_KEY,
        &[C::MediaService, C::MediaRelease],
        Some("emby"),
        Some(8096),
        H::ApiKeyHeader {
            name: "X-Emby-Token",
            prefix: None,
            secret_kind: None,
        },
    ));
    d.push(def(
        K::Plex,
        "plex",
        "Plex",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/plex.svg",
        API_KEY,
        &[C::MediaService, C::MediaRelease],
        Some("plex"),
        Some(32400),
        H::ApiKeyHeader {
            name: "X-Plex-Token",
            prefix: None,
            secret_kind: None,
        },
    ));

    let mut jellyseerr = def(
        K::Jellyseerr,
        "jellyseerr",
        "Jellyseerr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/jellyseerr.svg",
        API_KEY,
        &[C::MediaSearch, C::MediaRequest, C::Search],
        Some("jellyseerr"),
        Some(5055),
        H::Adapter,
    );
    jellyseerr.api_key_settings_path = Some("/settings");
    jellyseerr.docker_aliases = &["jellyseerr"];
    d.push(jellyseerr);

    let mut seerr = def(
        K::Seerr,
        "seerr",
        "Seerr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/seerr.svg",
        API_KEY,
        &[C::MediaSearch, C::MediaRequest, C::Search],
        Some("seerr"),
        Some(5055),
        H::Adapter,
    );
    seerr.api_key_settings_path = Some("/settings");
    seerr.onboarding_featured_order = Some(5);
    d.push(seerr);

    let mut overseerr = def(
        K::Overseerr,
        "overseerr",
        "Overseerr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/overseerr.svg",
        API_KEY,
        &[C::MediaSearch, C::MediaRequest, C::Search],
        Some("overseerr"),
        Some(5055),
        H::Adapter,
    );
    overseerr.api_key_settings_path = Some("/settings");
    overseerr.docker_aliases = &["overseerr"];
    d.push(overseerr);

    let mut pihole = def(
        K::PiHole,
        "piHole",
        "Pi-hole",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/pi-hole.svg",
        &[&[SK::ApiKey], &[]],
        &[C::DnsHole],
        Some("pi-hole"),
        Some(80),
        H::Adapter,
    );
    pihole.docker_aliases = &["pihole", "pi-hole"];
    d.push(pihole);

    let mut adguard = def(
        K::AdGuardHome,
        "adGuardHome",
        "AdGuard Home",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/adguard-home.svg",
        USER_PASS,
        &[C::DnsHole],
        Some("adguard-home"),
        Some(3000),
        H::Adapter,
    );
    adguard.docker_aliases = &["adguardhome", "adguard-home", "adguard"];
    d.push(adguard);

    d.push(def(
        K::TechnitiumDns,
        "technitiumDns",
        "Technitium DNS",
        "https://cdn.jsdelivr.net/gh/selfhst/icons/svg/technitium.svg",
        API_KEY_OR_USER_PASS,
        &[C::DnsHole],
        Some("technitium-dns"),
        Some(5380),
        H::Adapter,
    ));

    let mut ha = def(
        K::HomeAssistant,
        "homeAssistant",
        "Home Assistant",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/home-assistant.svg",
        API_KEY,
        &[C::SmartHomeServer, C::Calendar],
        Some("home-assistant"),
        Some(8123),
        H::Adapter,
    );
    ha.api_key_settings_path = Some("/profile/security");
    ha.docker_aliases = &["homeassistant", "home-assistant", "hass"];
    d.push(ha);

    let mut omv = def(
        K::Openmediavault,
        "openmediavault",
        "OpenMediaVault",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/openmediavault.svg",
        USER_PASS,
        &[C::HealthMonitoring],
        Some("open-media-vault"),
        Some(80),
        H::Adapter,
    );
    omv.optional_secret_kinds = &[SK::TotpSecret];
    omv.docker_aliases = &["omv"];
    d.push(omv);

    let mut dash = def(
        K::DashDot,
        "dashDot",
        "Dash.",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/dashdot.png",
        NONE,
        &[C::HealthMonitoring],
        Some("dash-dot"),
        Some(3001),
        H::None,
    );
    dash.docker_aliases = &["dashdot", "dash-dot", "dash."];
    d.push(dash);

    d.push(def(
        K::Glances,
        "glances",
        "Glances",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/glances.svg",
        NONE,
        &[C::HealthMonitoring],
        Some("glances"),
        Some(61208),
        H::None,
    ));

    static TDARR_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::ApiKey],
            auth: S::ApiKeyHeader {
                name: "X-Api-Key",
                prefix: None,
                secret_kind: None,
            },
        },
    ];
    d.push(def(
        K::Tdarr,
        "tdarr",
        "Tdarr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/tdarr.png",
        NONE_OR_API_KEY,
        &[C::MediaTranscoding],
        Some("tdarr"),
        Some(8265),
        H::Modes { modes: TDARR_MODES },
    ));
    d.push(def(
        K::Proxmox,
        "proxmox",
        "Proxmox",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/proxmox.svg",
        &[&[SK::Username, SK::TokenId, SK::ApiKey, SK::Realm]],
        &[C::HealthMonitoring],
        Some("proxmox"),
        Some(8006),
        H::Adapter,
    ));
    d.push(def(
        K::Nextcloud,
        "nextcloud",
        "Nextcloud",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/nextcloud.svg",
        USER_PASS,
        &[C::Calendar, C::Notifications],
        Some("nextcloud"),
        Some(443),
        H::Adapter,
    ));

    let mut unifi = def(
        K::UnifiController,
        "unifiController",
        "Unifi Controller",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/png/unifi.png",
        USER_PASS,
        &[C::NetworkController],
        Some("unifi-controller"),
        Some(8443),
        H::Adapter,
    );
    unifi.docker_aliases = &["unifi", "unifi-controller"];
    d.push(unifi);

    let mut opnsense = def(
        K::Opnsense,
        "opnsense",
        "OPNsense",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/opnsense.svg",
        &[&[SK::OpnsenseApiKey, SK::OpnsenseApiSecret]],
        &[C::Firewall],
        Some("opnsense"),
        Some(443),
        H::Basic {
            username_kind: Some(SK::OpnsenseApiKey),
            username: None,
            password_kind: Some(SK::OpnsenseApiSecret),
        },
    );
    opnsense.api_key_settings_path = Some("/system_usermanager.php");
    d.push(opnsense);

    static NTFY_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[SK::Topic],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::Topic, SK::ApiKey],
            auth: S::Bearer { secret_kind: None },
        },
    ];
    let mut ntfy = def(
        K::Ntfy,
        "ntfy",
        "ntfy",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/ntfy.svg",
        &[&[SK::Topic], &[SK::Topic, SK::ApiKey]],
        &[C::Notifications],
        Some("ntfy"),
        Some(80),
        H::Modes { modes: NTFY_MODES },
    );
    ntfy.api_key_settings_path = Some("/account");
    d.push(ntfy);

    d.push(def(
        K::Gotify,
        "gotify",
        "Gotify",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/gotify.svg",
        USER_PASS,
        &[C::Notifications],
        Some("gotify"),
        Some(80),
        H::Basic {
            username_kind: None,
            username: None,
            password_kind: None,
        },
    ));

    let mut ical = def(
        K::Ical,
        "ical",
        "iCal",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/ical.svg",
        &[&[SK::Url]],
        &[C::Calendar],
        Some("ical"),
        None,
        H::Adapter,
    );
    ical.docker_discoverable = false;
    d.push(ical);

    let mut anchor = def(
        K::Anchor,
        "anchor",
        "Anchor",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/anchor.svg",
        API_KEY,
        &[C::Notes],
        Some("anchor"),
        Some(8080),
        H::Bearer { secret_kind: None },
    );
    anchor.api_key_settings_path = Some("/settings");
    d.push(anchor);

    let mut truenas = def(
        K::Truenas,
        "truenas",
        "TrueNAS",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/truenas.svg",
        USER_PASS_OR_API_KEY,
        &[C::HealthMonitoring],
        Some("truenas"),
        Some(80),
        H::Adapter,
    );
    truenas.docker_aliases = &["truenas"];
    d.push(truenas);

    let mut synology = def(
        K::Synology,
        "synology",
        "Synology DiskStation",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/synology.svg",
        USER_PASS,
        &[C::HealthMonitoring],
        Some("synology"),
        Some(5000),
        H::Adapter,
    );
    synology.docker_aliases = &["synology", "diskstation"];
    d.push(synology);

    let mut unraid = def(
        K::Unraid,
        "unraid",
        "Unraid",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/unraid.svg",
        API_KEY,
        &[C::HealthMonitoring],
        Some("unraid"),
        Some(80),
        H::ApiKeyHeader {
            name: "x-api-key",
            prefix: None,
            secret_kind: None,
        },
    );
    unraid.api_key_settings_path = Some("/Settings/ManagementAccess");
    d.push(unraid);

    let mut coolify = def(
        K::Coolify,
        "coolify",
        "Coolify",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/coolify.svg",
        API_KEY,
        &[C::HealthMonitoring],
        Some("coolify"),
        Some(8000),
        H::Adapter,
    );
    coolify.api_key_settings_path = Some("/security/api-tokens");
    coolify.docker_aliases = &["coolify"];
    d.push(coolify);

    let mut immich = def(
        K::Immich,
        "immich",
        "Immich",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/immich.svg",
        API_KEY,
        &[C::PhotoService],
        Some("immich"),
        Some(2283),
        H::Adapter,
    );
    immich.api_key_settings_path = Some("/user-settings");
    d.push(immich);

    let mut paperless = def(
        K::PaperlessNgx,
        "paperlessNgx",
        "Paperless-ngx",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/paperless-ngx.svg",
        API_KEY,
        &[C::Documents],
        Some("paperless-ngx"),
        Some(8000),
        H::ApiKeyHeader {
            name: "Authorization",
            prefix: Some("Token "),
            secret_kind: None,
        },
    );
    paperless.docker_aliases = &["paperless-ngx", "paperless"];
    d.push(paperless);

    let mut thingport = def(
        K::Thingport,
        "thingport",
        "Thingport",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/thingport.svg",
        API_KEY,
        &[C::Miscellaneous, C::Search],
        Some("thingport"),
        Some(80),
        H::Bearer { secret_kind: None },
    );
    thingport.api_key_settings_path = Some("/profile");
    thingport.docker_aliases = &["thingport", "thingport-frontend"];
    d.push(thingport);

    let mut patchmon = def(
        K::Patchmon,
        "patchmon",
        "PatchMon",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/patchmon.svg",
        &[&[SK::PatchmonApiKey, SK::PatchmonApiSecret]],
        &[C::HealthMonitoring],
        Some("patchmon"),
        Some(3413),
        H::Basic {
            username_kind: Some(SK::PatchmonApiKey),
            username: None,
            password_kind: Some(SK::PatchmonApiSecret),
        },
    );
    patchmon.api_key_settings_path = Some("/settings/integrations");
    patchmon.docker_aliases = &["patchmon", "patch-mon"];
    d.push(patchmon);

    let mut tracearr = def(
        K::Tracearr,
        "tracearr",
        "Tracearr",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/tracearr.svg",
        API_KEY,
        &[C::MediaMonitoring],
        Some("tracearr"),
        Some(7040),
        H::Bearer { secret_kind: None },
    );
    tracearr.api_key_settings_path = Some("/settings");
    d.push(tracearr);

    let mut speedtest = def(
        K::SpeedtestTracker,
        "speedtestTracker",
        "Speedtest Tracker",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/png/speedtest-tracker.png",
        API_KEY,
        &[C::Speedtest],
        Some("speedtest-tracker"),
        Some(80),
        H::Adapter,
    );
    speedtest.api_key_settings_path = Some("/admin/api-tokens");
    speedtest.docker_aliases = &["speedtest-tracker"];
    d.push(speedtest);

    static UPTIME_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::Slug],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::ApiKey],
            auth: S::Basic {
                username_kind: None,
                username: Some(""),
                password_kind: Some(SK::ApiKey),
            },
        },
        AuthModeSpec {
            when: &[SK::Slug, SK::ApiKey],
            auth: S::Basic {
                username_kind: None,
                username: Some(""),
                password_kind: Some(SK::ApiKey),
            },
        },
    ];
    let mut uptime = def(
        K::UptimeKuma,
        "uptimeKuma",
        "Uptime Kuma",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/uptime-kuma.svg",
        &[&[], &[SK::Slug], &[SK::Slug, SK::ApiKey], &[SK::ApiKey]],
        &[C::UptimeMonitoring],
        Some("uptime-kuma"),
        Some(3001),
        H::Modes {
            modes: UPTIME_MODES,
        },
    );
    uptime.api_key_settings_path = Some("/settings/api-keys");
    uptime.docker_aliases = &["uptime-kuma"];
    d.push(uptime);

    let mut abs = def(
        K::Audiobookshelf,
        "audiobookshelf",
        "Audiobookshelf",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/audiobookshelf.svg",
        API_KEY,
        &[C::MediaLibrary],
        Some("audiobookshelf"),
        Some(13378),
        H::Adapter,
    );
    abs.api_key_settings_path = Some("/account");
    abs.docker_aliases = &["audiobookshelf"];
    d.push(abs);

    let mut navidrome = def(
        K::Navidrome,
        "navidrome",
        "Navidrome",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/navidrome.svg",
        USER_PASS,
        &[C::MediaLibrary, C::MediaService],
        Some("navidrome"),
        Some(4533),
        H::Adapter,
    );
    navidrome.docker_aliases = &["navidrome"];
    d.push(navidrome);

    let mut umami = def(
        K::Umami,
        "umami",
        "Umami",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/umami.svg",
        USER_PASS_OR_API_KEY,
        &[C::Analytics],
        Some("umami"),
        Some(3000),
        H::Adapter,
    );
    umami.default_url = Some("https://api.umami.is/v1");
    d.push(umami);

    static PEANUT_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::Username, SK::Password],
            auth: S::Basic {
                username_kind: None,
                username: None,
                password_kind: None,
            },
        },
    ];
    d.push(def(
        K::PeaNut,
        "peaNut",
        "PeaNUT",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/peanut.svg",
        &[&[SK::Username, SK::Password], &[]],
        &[C::Ups],
        Some("peanut"),
        Some(8080),
        H::Modes {
            modes: PEANUT_MODES,
        },
    ));
    d.push(def(
        K::Beszel,
        "beszel",
        "Beszel",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/beszel.svg",
        USER_PASS,
        &[C::Beszel],
        Some("beszel"),
        Some(8090),
        H::Adapter,
    ));

    static GLUETUN_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::ApiKey],
            auth: S::ApiKeyHeader {
                name: "X-API-Key",
                prefix: None,
                secret_kind: None,
            },
        },
        AuthModeSpec {
            when: &[SK::Username, SK::Password],
            auth: S::Basic {
                username_kind: None,
                username: None,
                password_kind: None,
            },
        },
    ];
    d.push(def(
        K::Gluetun,
        "gluetun",
        "Gluetun",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/gluetun.svg",
        &[&[SK::Username, SK::Password], &[SK::ApiKey], &[]],
        &[C::Vpn],
        Some("gluetun"),
        Some(8000),
        H::Modes {
            modes: GLUETUN_MODES,
        },
    ));

    static TRAEFIK_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::ApiKey],
            auth: S::Bearer { secret_kind: None },
        },
        AuthModeSpec {
            when: &[SK::Username, SK::Password],
            auth: S::Basic {
                username_kind: None,
                username: None,
                password_kind: None,
            },
        },
    ];
    d.push(def(
        K::Traefik,
        "traefik",
        "Traefik",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/traefik.svg",
        &[&[], &[SK::Username, SK::Password], &[SK::ApiKey]],
        &[C::ReverseProxy],
        Some("traefik"),
        Some(8080),
        H::Modes {
            modes: TRAEFIK_MODES,
        },
    ));

    static ARCHIVE_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
        AuthModeSpec {
            when: &[SK::Username, SK::Password],
            auth: S::Basic {
                username_kind: None,
                username: None,
                password_kind: None,
            },
        },
    ];
    d.push(def(
        K::ArchiveTeamWarrior,
        "archiveTeamWarrior",
        "ArchiveTeam Warrior",
        "https://cdn.jsdelivr.net/gh/selfhst/icons/png/archiveteam-warrior.png",
        NONE_OR_USER_PASS,
        &[C::Archiving],
        Some("archiveteam-warrior"),
        Some(8001),
        H::Modes {
            modes: ARCHIVE_MODES,
        },
    ));

    let mut wazuh = def(
        K::Wazuh,
        "wazuh",
        "Wazuh",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/wazuh.svg",
        &[
            &[SK::Username, SK::Password],
            &[
                SK::Username,
                SK::Password,
                SK::WazuhIndexerUrl,
                SK::WazuhIndexerUsername,
                SK::WazuhIndexerPassword,
            ],
            &[SK::WazuhIndexerUsername, SK::WazuhIndexerPassword],
        ],
        &[C::Security, C::Notifications],
        Some("wazuh"),
        Some(55000),
        H::Adapter,
    );
    wazuh.docker_aliases = &["wazuh", "wazuh-manager", "wazuh.manager"];
    d.push(wazuh);

    static WUD_MODES: &[AuthModeSpec] = &[
        AuthModeSpec {
            when: &[SK::PersonalAccessToken],
            auth: S::Bearer {
                secret_kind: Some(SK::PersonalAccessToken),
            },
        },
        AuthModeSpec {
            when: &[SK::Username, SK::Password],
            auth: S::Basic {
                username_kind: None,
                username: None,
                password_kind: None,
            },
        },
        AuthModeSpec {
            when: &[],
            auth: S::None,
        },
    ];
    let mut wud = def(
        K::Wud,
        "wud",
        "What's Up Docker",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/whats-up-docker.svg",
        &[
            &[SK::PersonalAccessToken],
            &[SK::Username, SK::Password],
            &[],
        ],
        &[C::HealthMonitoring],
        Some("whats-up-docker"),
        Some(3000),
        H::Modes { modes: WUD_MODES },
    );
    wud.api_key_settings_path = Some("/profile");
    d.push(wud);

    d.push(def(
        K::Llamacpp,
        "llamacpp",
        "llama.cpp",
        "https://cdn.jsdelivr.net/gh/selfhst/icons/svg/llama-cpp.svg",
        NONE,
        &[C::SystemMonitoring],
        Some("llama-cpp"),
        Some(8080),
        H::None,
    ));

    let mut mock = def(
        K::Mock,
        "mock",
        "Mock",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/vitest.svg",
        NONE,
        &[
            C::Calendar,
            C::DnsHole,
            C::DownloadClient,
            C::HealthMonitoring,
            C::IndexerManager,
            C::MediaRelease,
            C::MediaRequest,
            C::MediaService,
            C::MediaTranscoding,
            C::NetworkController,
            C::Notifications,
            C::SmartHomeServer,
            C::Ups,
            C::UptimeMonitoring,
            C::MediaOrganizer,
            C::ServiceVersion,
        ],
        None,
        None,
        H::None,
    );
    mock.docker_discoverable = false;
    mock.onboarding_hidden = true;
    d.push(mock);

    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_92_integrations_defined_and_roundtrip() {
        let defs = all_integration_defs();
        assert_eq!(defs.len(), 92);
        for d in defs {
            assert_eq!(IntegrationKind::from_str_opt(d.kind_str), Some(d.kind));
        }
    }

    #[test]
    fn api_key_url_construction() {
        let url = get_integration_api_key_url("http://sonarr:8989/base/", IntegrationKind::Sonarr)
            .unwrap();
        assert_eq!(url, "http://sonarr:8989/base/settings/general");
    }
}
