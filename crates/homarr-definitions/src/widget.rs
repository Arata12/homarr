use serde::{Deserialize, Serialize};

use crate::integration::{get_integration_kinds_by_category, IntegrationCategory, IntegrationKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetKind {
    #[serde(rename = "stats")]
    Stats,
    #[serde(rename = "clock")]
    Clock,
    #[serde(rename = "weather")]
    Weather,
    #[serde(rename = "airQuality")]
    AirQuality,
    #[serde(rename = "countdown")]
    Countdown,
    #[serde(rename = "timer")]
    Timer,
    #[serde(rename = "app")]
    App,
    #[serde(rename = "iframe")]
    Iframe,
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "notebook")]
    Notebook,
    #[serde(rename = "anchorNote")]
    AnchorNote,
    #[serde(rename = "dnsHoleSummary")]
    DnsHoleSummary,
    #[serde(rename = "dnsHoleControls")]
    DnsHoleControls,
    #[serde(rename = "smartHome-entityState")]
    SmartHomeEntityState,
    #[serde(rename = "smartHome-executeAutomation")]
    SmartHomeExecuteAutomation,
    #[serde(rename = "smartHome-controls")]
    SmartHomeControls,
    #[serde(rename = "stockPrice")]
    StockPrice,
    #[serde(rename = "mediaServer")]
    MediaServer,
    #[serde(rename = "calendar")]
    Calendar,
    #[serde(rename = "downloads")]
    Downloads,
    #[serde(rename = "mediaRequests-requestList")]
    MediaRequestsRequestList,
    #[serde(rename = "mediaRequests-requestStats")]
    MediaRequestsRequestStats,
    #[serde(rename = "mediaTranscoding")]
    MediaTranscoding,
    #[serde(rename = "mediaMissing")]
    MediaMissing,
    #[serde(rename = "minecraftServerStatus")]
    MinecraftServerStatus,
    #[serde(rename = "networkControllerSummary")]
    NetworkControllerSummary,
    #[serde(rename = "networkControllerStatus")]
    NetworkControllerStatus,
    #[serde(rename = "rssFeed")]
    RssFeed,
    #[serde(rename = "bookmarks")]
    Bookmarks,
    #[serde(rename = "indexerManager")]
    IndexerManager,
    #[serde(rename = "healthMonitoring")]
    HealthMonitoring,
    #[serde(rename = "releases")]
    Releases,
    #[serde(rename = "mediaReleases")]
    MediaReleases,
    #[serde(rename = "dockerContainers")]
    DockerContainers,
    #[serde(rename = "firewall")]
    Firewall,
    #[serde(rename = "notifications")]
    Notifications,
    #[serde(rename = "systemResources")]
    SystemResources,
    #[serde(rename = "coolify")]
    Coolify,
    #[serde(rename = "systemDisks")]
    SystemDisks,
    #[serde(rename = "timetable")]
    Timetable,
    #[serde(rename = "immich-serverStats")]
    ImmichServerStats,
    #[serde(rename = "immich-albumCarousel")]
    ImmichAlbumCarousel,
    #[serde(rename = "paperlessNgx")]
    PaperlessNgx,
    #[serde(rename = "thingport-libraryStats")]
    ThingportLibraryStats,
    #[serde(rename = "thingport-models")]
    ThingportModels,
    #[serde(rename = "thingport-featuredModel")]
    ThingportFeaturedModel,
    #[serde(rename = "thingport-activityHeatmap")]
    ThingportActivityHeatmap,
    #[serde(rename = "patchmon")]
    Patchmon,
    #[serde(rename = "bazarr")]
    Bazarr,
    #[serde(rename = "tracearr")]
    Tracearr,
    #[serde(rename = "speedtestTracker")]
    SpeedtestTracker,
    #[serde(rename = "uptimeKuma")]
    UptimeKuma,
    #[serde(rename = "audioStats")]
    AudioStats,
    #[serde(rename = "umami")]
    Umami,
    #[serde(rename = "vpn")]
    Vpn,
    #[serde(rename = "archiveTeamWarrior")]
    ArchiveTeamWarrior,
    #[serde(rename = "ups")]
    Ups,
    #[serde(rename = "beszelSystemTable")]
    BeszelSystemTable,
    #[serde(rename = "beszelSystemGrid")]
    BeszelSystemGrid,
    #[serde(rename = "beszelAlerts")]
    BeszelAlerts,
    #[serde(rename = "beszelSystemStats")]
    BeszelSystemStats,
    #[serde(rename = "traefik")]
    Traefik,
    #[serde(rename = "customApi")]
    CustomApi,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "wud")]
    Wud,
    #[serde(rename = "llamacpp")]
    Llamacpp,
    #[serde(rename = "wazuh")]
    Wazuh,
}

impl WidgetKind {
    pub const ALL: [Self; 67] = [
        Self::Stats,
        Self::Clock,
        Self::Weather,
        Self::AirQuality,
        Self::Countdown,
        Self::Timer,
        Self::App,
        Self::Iframe,
        Self::Video,
        Self::Notebook,
        Self::AnchorNote,
        Self::DnsHoleSummary,
        Self::DnsHoleControls,
        Self::SmartHomeEntityState,
        Self::SmartHomeExecuteAutomation,
        Self::SmartHomeControls,
        Self::StockPrice,
        Self::MediaServer,
        Self::Calendar,
        Self::Downloads,
        Self::MediaRequestsRequestList,
        Self::MediaRequestsRequestStats,
        Self::MediaTranscoding,
        Self::MediaMissing,
        Self::MinecraftServerStatus,
        Self::NetworkControllerSummary,
        Self::NetworkControllerStatus,
        Self::RssFeed,
        Self::Bookmarks,
        Self::IndexerManager,
        Self::HealthMonitoring,
        Self::Releases,
        Self::MediaReleases,
        Self::DockerContainers,
        Self::Firewall,
        Self::Notifications,
        Self::SystemResources,
        Self::Coolify,
        Self::SystemDisks,
        Self::Timetable,
        Self::ImmichServerStats,
        Self::ImmichAlbumCarousel,
        Self::PaperlessNgx,
        Self::ThingportLibraryStats,
        Self::ThingportModels,
        Self::ThingportFeaturedModel,
        Self::ThingportActivityHeatmap,
        Self::Patchmon,
        Self::Bazarr,
        Self::Tracearr,
        Self::SpeedtestTracker,
        Self::UptimeKuma,
        Self::AudioStats,
        Self::Umami,
        Self::Vpn,
        Self::ArchiveTeamWarrior,
        Self::Ups,
        Self::BeszelSystemTable,
        Self::BeszelSystemGrid,
        Self::BeszelAlerts,
        Self::BeszelSystemStats,
        Self::Traefik,
        Self::CustomApi,
        Self::Assistant,
        Self::Wud,
        Self::Llamacpp,
        Self::Wazuh,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Clock => "clock",
            Self::Weather => "weather",
            Self::AirQuality => "airQuality",
            Self::Countdown => "countdown",
            Self::Timer => "timer",
            Self::App => "app",
            Self::Iframe => "iframe",
            Self::Video => "video",
            Self::Notebook => "notebook",
            Self::AnchorNote => "anchorNote",
            Self::DnsHoleSummary => "dnsHoleSummary",
            Self::DnsHoleControls => "dnsHoleControls",
            Self::SmartHomeEntityState => "smartHome-entityState",
            Self::SmartHomeExecuteAutomation => "smartHome-executeAutomation",
            Self::SmartHomeControls => "smartHome-controls",
            Self::StockPrice => "stockPrice",
            Self::MediaServer => "mediaServer",
            Self::Calendar => "calendar",
            Self::Downloads => "downloads",
            Self::MediaRequestsRequestList => "mediaRequests-requestList",
            Self::MediaRequestsRequestStats => "mediaRequests-requestStats",
            Self::MediaTranscoding => "mediaTranscoding",
            Self::MediaMissing => "mediaMissing",
            Self::MinecraftServerStatus => "minecraftServerStatus",
            Self::NetworkControllerSummary => "networkControllerSummary",
            Self::NetworkControllerStatus => "networkControllerStatus",
            Self::RssFeed => "rssFeed",
            Self::Bookmarks => "bookmarks",
            Self::IndexerManager => "indexerManager",
            Self::HealthMonitoring => "healthMonitoring",
            Self::Releases => "releases",
            Self::MediaReleases => "mediaReleases",
            Self::DockerContainers => "dockerContainers",
            Self::Firewall => "firewall",
            Self::Notifications => "notifications",
            Self::SystemResources => "systemResources",
            Self::Coolify => "coolify",
            Self::SystemDisks => "systemDisks",
            Self::Timetable => "timetable",
            Self::ImmichServerStats => "immich-serverStats",
            Self::ImmichAlbumCarousel => "immich-albumCarousel",
            Self::PaperlessNgx => "paperlessNgx",
            Self::ThingportLibraryStats => "thingport-libraryStats",
            Self::ThingportModels => "thingport-models",
            Self::ThingportFeaturedModel => "thingport-featuredModel",
            Self::ThingportActivityHeatmap => "thingport-activityHeatmap",
            Self::Patchmon => "patchmon",
            Self::Bazarr => "bazarr",
            Self::Tracearr => "tracearr",
            Self::SpeedtestTracker => "speedtestTracker",
            Self::UptimeKuma => "uptimeKuma",
            Self::AudioStats => "audioStats",
            Self::Umami => "umami",
            Self::Vpn => "vpn",
            Self::ArchiveTeamWarrior => "archiveTeamWarrior",
            Self::Ups => "ups",
            Self::BeszelSystemTable => "beszelSystemTable",
            Self::BeszelSystemGrid => "beszelSystemGrid",
            Self::BeszelAlerts => "beszelAlerts",
            Self::BeszelSystemStats => "beszelSystemStats",
            Self::Traefik => "traefik",
            Self::CustomApi => "customApi",
            Self::Assistant => "assistant",
            Self::Wud => "wud",
            Self::Llamacpp => "llamacpp",
            Self::Wazuh => "wazuh",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

pub fn all_widget_kinds() -> &'static [WidgetKind] {
    &WidgetKind::ALL
}

pub fn widget_default_size(kind: WidgetKind) -> (i32, i32) {
    match kind {
        WidgetKind::App => (1, 1),
        WidgetKind::Clock
        | WidgetKind::Weather
        | WidgetKind::AirQuality
        | WidgetKind::Countdown
        | WidgetKind::Timer
        | WidgetKind::StockPrice
        | WidgetKind::MinecraftServerStatus
        | WidgetKind::Releases
        | WidgetKind::DnsHoleControls
        | WidgetKind::NetworkControllerStatus
        | WidgetKind::SmartHomeEntityState
        | WidgetKind::SmartHomeExecuteAutomation => (2, 1),
        WidgetKind::Bookmarks
        | WidgetKind::SmartHomeControls
        | WidgetKind::AudioStats
        | WidgetKind::PaperlessNgx
        | WidgetKind::ThingportLibraryStats
        | WidgetKind::ThingportFeaturedModel
        | WidgetKind::Patchmon
        | WidgetKind::MediaRequestsRequestStats
        | WidgetKind::MediaMissing
        | WidgetKind::Bazarr
        | WidgetKind::Vpn
        | WidgetKind::Ups => (2, 2),
        WidgetKind::DnsHoleSummary
        | WidgetKind::Notebook
        | WidgetKind::AnchorNote
        | WidgetKind::Calendar
        | WidgetKind::CustomApi
        | WidgetKind::MediaServer
        | WidgetKind::MediaRequestsRequestList
        | WidgetKind::MediaReleases
        | WidgetKind::MediaTranscoding
        | WidgetKind::HealthMonitoring
        | WidgetKind::SystemResources
        | WidgetKind::SystemDisks
        | WidgetKind::Firewall
        | WidgetKind::Traefik
        | WidgetKind::NetworkControllerSummary
        | WidgetKind::Notifications
        | WidgetKind::RssFeed
        | WidgetKind::Stats
        | WidgetKind::SpeedtestTracker
        | WidgetKind::ImmichServerStats
        | WidgetKind::Umami
        | WidgetKind::ArchiveTeamWarrior
        | WidgetKind::Wud
        | WidgetKind::Llamacpp
        | WidgetKind::Coolify
        | WidgetKind::Tracearr
        | WidgetKind::BeszelAlerts
        | WidgetKind::BeszelSystemStats
        | WidgetKind::Timetable => (3, 2),
        WidgetKind::IndexerManager | WidgetKind::UptimeKuma => (2, 3),
        WidgetKind::ThingportModels => (3, 3),
        WidgetKind::Downloads
        | WidgetKind::DockerContainers
        | WidgetKind::BeszelSystemTable
        | WidgetKind::BeszelSystemGrid
        | WidgetKind::ImmichAlbumCarousel => (4, 2),
        WidgetKind::Iframe | WidgetKind::Video | WidgetKind::Wazuh => (4, 3),
        WidgetKind::ThingportActivityHeatmap => (6, 2),
        WidgetKind::Assistant => (10, 4),
    }
}

pub fn widget_requires_integration(kind: WidgetKind) -> bool {
    if widget_integration_support(kind).is_none() {
        return false;
    }
    !matches!(kind, WidgetKind::Calendar | WidgetKind::Releases)
}

pub fn widget_max_integrations(kind: WidgetKind) -> Option<usize> {
    match kind {
        WidgetKind::SmartHomeEntityState
        | WidgetKind::SmartHomeExecuteAutomation
        | WidgetKind::SmartHomeControls
        | WidgetKind::MediaTranscoding
        | WidgetKind::ImmichServerStats
        | WidgetKind::ImmichAlbumCarousel
        | WidgetKind::PaperlessNgx
        | WidgetKind::ThingportLibraryStats
        | WidgetKind::ThingportModels
        | WidgetKind::ThingportFeaturedModel
        | WidgetKind::ThingportActivityHeatmap
        | WidgetKind::Patchmon
        | WidgetKind::Bazarr
        | WidgetKind::AudioStats
        | WidgetKind::Umami
        | WidgetKind::ArchiveTeamWarrior
        | WidgetKind::AnchorNote
        | WidgetKind::Wud
        | WidgetKind::Llamacpp
        | WidgetKind::Wazuh => Some(1),
        _ => None,
    }
}

pub fn widget_integration_support(kind: WidgetKind) -> Option<Vec<IntegrationKind>> {
    use IntegrationCategory::*;
    use IntegrationKind as IK;
    let kinds = match kind {
        WidgetKind::Stats => crate::integration::all_integration_defs()
            .iter()
            .filter(|d| d.kind != IK::Ical)
            .map(|d| d.kind)
            .collect(),
        WidgetKind::DnsHoleSummary | WidgetKind::DnsHoleControls => {
            get_integration_kinds_by_category(DnsHole)
        }
        WidgetKind::SmartHomeEntityState
        | WidgetKind::SmartHomeExecuteAutomation
        | WidgetKind::SmartHomeControls => get_integration_kinds_by_category(SmartHomeServer),
        WidgetKind::MediaServer => get_integration_kinds_by_category(MediaService),
        WidgetKind::Calendar => get_integration_kinds_by_category(Calendar),
        WidgetKind::Releases => get_integration_kinds_by_category(ServiceVersion),
        WidgetKind::Downloads => get_integration_kinds_by_category(DownloadClient),
        WidgetKind::MediaRequestsRequestList | WidgetKind::MediaRequestsRequestStats => {
            get_integration_kinds_by_category(MediaRequest)
        }
        WidgetKind::MediaTranscoding => get_integration_kinds_by_category(MediaTranscoding),
        WidgetKind::MediaMissing => get_integration_kinds_by_category(MediaOrganizer),
        WidgetKind::NetworkControllerSummary | WidgetKind::NetworkControllerStatus => {
            get_integration_kinds_by_category(NetworkController)
        }
        WidgetKind::IndexerManager => get_integration_kinds_by_category(IndexerManager),
        WidgetKind::HealthMonitoring => vec![
            IK::Openmediavault,
            IK::DashDot,
            IK::Truenas,
            IK::Unraid,
            IK::Glances,
            IK::Synology,
            IK::Proxmox,
            IK::Mock,
        ],
        WidgetKind::Firewall => {
            let mut v = get_integration_kinds_by_category(Firewall);
            v.push(IK::Mock);
            v
        }
        WidgetKind::Notifications => get_integration_kinds_by_category(Notifications),
        WidgetKind::MediaReleases => vec![IK::Mock, IK::Emby, IK::Jellyfin, IK::Plex],
        WidgetKind::SystemResources => vec![
            IK::Openmediavault,
            IK::DashDot,
            IK::Truenas,
            IK::Unraid,
            IK::Glances,
            IK::Synology,
            IK::Mock,
        ],
        WidgetKind::SystemDisks => vec![
            IK::DashDot,
            IK::Openmediavault,
            IK::Truenas,
            IK::Unraid,
            IK::Synology,
            IK::Mock,
        ],
        WidgetKind::BeszelAlerts
        | WidgetKind::BeszelSystemGrid
        | WidgetKind::BeszelSystemStats
        | WidgetKind::BeszelSystemTable => vec![IK::Beszel, IK::Mock],
        WidgetKind::Coolify => vec![IK::Coolify, IK::Mock],
        WidgetKind::ImmichServerStats | WidgetKind::ImmichAlbumCarousel => {
            vec![IK::Immich, IK::Mock]
        }
        WidgetKind::PaperlessNgx => vec![IK::PaperlessNgx, IK::Mock],
        WidgetKind::ThingportLibraryStats
        | WidgetKind::ThingportModels
        | WidgetKind::ThingportFeaturedModel
        | WidgetKind::ThingportActivityHeatmap => vec![IK::Thingport, IK::Mock],
        WidgetKind::Patchmon => vec![IK::Patchmon, IK::Mock],
        WidgetKind::Bazarr => vec![IK::Bazarr, IK::Mock],
        WidgetKind::Tracearr => vec![IK::Tracearr, IK::Mock],
        WidgetKind::SpeedtestTracker => vec![IK::SpeedtestTracker, IK::Mock],
        WidgetKind::UptimeKuma => vec![IK::UptimeKuma, IK::Mock],
        WidgetKind::AudioStats => vec![IK::Navidrome, IK::Audiobookshelf, IK::Mock],
        WidgetKind::Umami => vec![IK::Umami, IK::Mock],
        WidgetKind::Vpn => {
            let mut v = get_integration_kinds_by_category(Vpn);
            v.push(IK::Mock);
            v
        }
        WidgetKind::Ups => get_integration_kinds_by_category(Ups),
        WidgetKind::ArchiveTeamWarrior => vec![IK::ArchiveTeamWarrior, IK::Mock],
        WidgetKind::AnchorNote => vec![IK::Anchor, IK::Mock],
        WidgetKind::Traefik => vec![IK::Traefik, IK::Mock],
        WidgetKind::Wud => vec![IK::Wud, IK::Mock],
        WidgetKind::Llamacpp => vec![IK::Llamacpp, IK::Mock],
        WidgetKind::Wazuh => vec![IK::Wazuh, IK::Mock],
        WidgetKind::Clock
        | WidgetKind::Weather
        | WidgetKind::AirQuality
        | WidgetKind::Countdown
        | WidgetKind::Timer
        | WidgetKind::App
        | WidgetKind::Iframe
        | WidgetKind::Video
        | WidgetKind::Notebook
        | WidgetKind::StockPrice
        | WidgetKind::MinecraftServerStatus
        | WidgetKind::RssFeed
        | WidgetKind::Bookmarks
        | WidgetKind::DockerContainers
        | WidgetKind::Timetable
        | WidgetKind::CustomApi
        | WidgetKind::Assistant => return None,
    };
    Some(kinds)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetIntegrationIssue {
    IntegrationNotSupported,
    IntegrationRequired,
    IncompatibleIntegration(Vec<IntegrationKind>),
    IntegrationLimit(usize),
}

pub fn get_widget_integration_issue(
    widget_kind: WidgetKind,
    selected: &[IntegrationKind],
) -> Option<WidgetIntegrationIssue> {
    if let Some(limit) = widget_max_integrations(widget_kind) {
        if selected.len() > limit {
            return Some(WidgetIntegrationIssue::IntegrationLimit(limit));
        }
    }
    let supported = widget_integration_support(widget_kind);
    match supported {
        None => {
            if !selected.is_empty() {
                return Some(WidgetIntegrationIssue::IntegrationNotSupported);
            }
            None
        }
        Some(allowed) => {
            if selected.is_empty() && widget_requires_integration(widget_kind) {
                return Some(WidgetIntegrationIssue::IntegrationRequired);
            }
            let incompatible: Vec<IntegrationKind> = selected
                .iter()
                .copied()
                .filter(|k| !allowed.contains(k))
                .collect();
            if incompatible.is_empty() {
                None
            } else {
                Some(WidgetIntegrationIssue::IncompatibleIntegration(
                    incompatible,
                ))
            }
        }
    }
}
