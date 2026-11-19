use homarr_definitions::widget::{
    all_widget_kinds, widget_default_size, widget_integration_support, WidgetKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetMetadata {
    pub kind: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub category: String,
    pub default_width: i32,
    pub default_height: i32,
    pub supported_integrations: Vec<String>,
    pub default_options: Value,
}

pub fn all_widget_metadata() -> Vec<WidgetMetadata> {
    all_widget_kinds()
        .iter()
        .copied()
        .map(get_widget_metadata)
        .collect()
}

pub fn get_widget_metadata(kind: WidgetKind) -> WidgetMetadata {
    let (w, h) = widget_default_size(kind);
    let supported = widget_integration_support(kind)
        .unwrap_or_default()
        .into_iter()
        .map(|k| k.as_str().to_string())
        .collect();
    let (name, description, icon, category, default_options) = widget_details(kind);
    WidgetMetadata {
        kind: kind.as_str().to_string(),
        name: name.to_string(),
        description: description.to_string(),
        icon: icon.to_string(),
        category: category.to_string(),
        default_width: w,
        default_height: h,
        supported_integrations: supported,
        default_options,
    }
}

/// A selectable choice for `select` / `multiSelect` option fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionChoice {
    pub value: String,
    pub label: String,
}

/// Describes one editable widget option so the board editor can generate a form field.
///
/// `field_type` is one of: `text`, `textarea`, `number`, `slider`, `switch`, `select`,
/// `multiSelect`, `textList`, `app`, `location`, `json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionField {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub default_value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub choices: Vec<OptionChoice>,
}

fn choices(values: &[&str]) -> Vec<OptionChoice> {
    values
        .iter()
        .map(|v| OptionChoice {
            value: (*v).to_string(),
            label: (*v).to_string(),
        })
        .collect()
}

/// Builds the option form definition of a widget. Field types are inferred from the widget's
/// default options and refined by explicit overrides (ranges, enumerations, pickers).
pub fn widget_option_fields(kind: WidgetKind) -> Vec<OptionField> {
    let defaults = get_widget_metadata(kind).default_options;
    let Some(map) = defaults.as_object() else {
        return Vec::new();
    };
    map.iter()
        .map(|(key, default)| build_option_field(kind, key, default))
        .collect()
}

fn build_option_field(kind: WidgetKind, key: &str, default: &Value) -> OptionField {
    use WidgetKind as W;
    let mut field = OptionField {
        key: key.to_string(),
        field_type: match default {
            Value::Bool(_) => "switch",
            Value::Number(_) => "number",
            Value::String(_) => "text",
            Value::Array(a) if !a.is_empty() && a.iter().all(Value::is_string) => "textList",
            Value::Object(o) if o.contains_key("latitude") && o.contains_key("longitude") => {
                "location"
            }
            _ => "json",
        }
        .to_string(),
        default_value: default.clone(),
        min: None,
        max: None,
        step: None,
        choices: Vec::new(),
    };
    match (kind, key) {
        (W::App, "appId") => field.field_type = "app".into(),
        (W::Notebook, "content") | (W::Assistant, "placeholder") => {
            field.field_type = "textarea".into();
        }
        (W::SmartHomeControls, "entityIds") | (W::RssFeed, "feedUrls") => {
            field.field_type = "textList".into();
        }
        (W::Bookmarks, "items") | (W::Stats, "metrics") => {
            field.field_type = "json".into();
        }
        (W::Bookmarks, "layout") => {
            field.field_type = "select".into();
            field.choices = choices(&["grid", "list"]);
        }
        (W::StockPrice, "timeRange") => {
            field.field_type = "select".into();
            field.choices = choices(&["1d", "5d", "1mo", "3mo", "6mo", "1y"]);
        }
        (W::Releases, "provider") => {
            field.field_type = "select".into();
            field.choices = choices(&["github", "gitlab", "dockerhub", "codeberg"]);
        }
        (W::Downloads, "columns") => {
            field.field_type = "multiSelect".into();
            field.choices = choices(&["name", "progress", "speed", "size", "eta", "state"]);
        }
        (W::RssFeed, "maximumAmountOfPosts") => {
            field.field_type = "slider".into();
            (field.min, field.max, field.step) = (Some(1.0), Some(50.0), Some(1.0));
        }
        (W::Timer, "durationSeconds") => {
            field.min = Some(1.0);
            field.max = Some(86_400.0);
        }
        _ => {}
    }
    field
}

/// Normalizes submitted option values against the widget's field definitions: coerces types
/// and drops values that don't match. Unknown keys are kept untouched.
pub fn sanitize_widget_options(kind: WidgetKind, options: &Value) -> Value {
    let Some(obj) = options.as_object() else {
        return Value::Object(Default::default());
    };
    let fields = widget_option_fields(kind);
    let mut out = obj.clone();
    for f in &fields {
        let Some(v) = obj.get(&f.key) else { continue };
        let ok = match f.field_type.as_str() {
            "switch" => v.is_boolean(),
            "number" | "slider" => v
                .as_f64()
                .is_some_and(|n| f.min.is_none_or(|m| n >= m) && f.max.is_none_or(|m| n <= m)),
            "text" | "textarea" | "app" => v.is_string(),
            "select" => v
                .as_str()
                .is_some_and(|s| f.choices.is_empty() || f.choices.iter().any(|c| c.value == s)),
            "multiSelect" => v.as_array().is_some_and(|a| {
                a.iter().all(|e| {
                    e.as_str()
                        .is_some_and(|s| f.choices.iter().any(|c| c.value == s))
                })
            }),
            "textList" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
            "location" => {
                v.get("latitude")
                    .and_then(Value::as_f64)
                    .is_some_and(|n| (-90.0..=90.0).contains(&n))
                    && v.get("longitude")
                        .and_then(Value::as_f64)
                        .is_some_and(|n| (-180.0..=180.0).contains(&n))
                    && v.get("name").is_some_and(Value::is_string)
            }
            _ => true,
        };
        if !ok {
            out.insert(f.key.clone(), f.default_value.clone());
        }
    }
    Value::Object(out)
}

fn widget_details(
    kind: WidgetKind,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Value,
) {
    use WidgetKind as W;
    match kind {
        W::App => (
            "Application",
            "Shortcut card linking to an application with live status ping",
            "apps",
            "general",
            json!({ "appId": "", "openInNewTab": true, "showTitle": true, "showDescription": false }),
        ),
        W::Bookmarks => (
            "Bookmarks",
            "Organized list of web bookmarks and shortcuts",
            "bookmarks",
            "general",
            json!({ "title": "Bookmarks", "layout": "grid", "items": [] }),
        ),
        W::Clock => (
            "Clock",
            "Displays the current time and date in 12h or 24h format",
            "clock",
            "utilities",
            json!({ "is24HourFormat": true, "showDate": true, "showSeconds": false, "customTitle": "", "timezone": "UTC" }),
        ),
        W::Weather => (
            "Weather",
            "Current weather and weekly forecast powered by Open-Meteo",
            "cloud-sun",
            "weather",
            json!({ "location": { "name": "Zurich", "latitude": 47.3769, "longitude": 8.5417 }, "isFahrenheit": false, "disableCityName": false, "showCurrentWindSpeed": true }),
        ),
        W::AirQuality => (
            "Air Quality",
            "Air quality index and pollutant levels from Open-Meteo",
            "wind",
            "weather",
            json!({ "location": { "name": "Zurich", "latitude": 47.3769, "longitude": 8.5417 } }),
        ),
        W::Countdown => (
            "Countdown",
            "Counts down to a target date and event",
            "hourglass",
            "utilities",
            json!({ "title": "Countdown", "date": "2027-01-01T00:00:00Z" }),
        ),
        W::Timer => (
            "Timer",
            "Interactive Pomodoro or interval timer",
            "stopwatch",
            "utilities",
            json!({ "title": "Timer", "durationSeconds": 1500 }),
        ),
        W::Notebook => (
            "Notebook",
            "Markdown-capable quick notes right on your board",
            "notes",
            "utilities",
            json!({ "content": "# Homelab Notes\nWelcome to Homarr Rust.", "showToolbar": true }),
        ),
        W::AnchorNote => (
            "Anchor Note",
            "Synced quick note backed by Anchor integration",
            "note",
            "utilities",
            json!({ "noteId": "" }),
        ),
        W::Iframe => (
            "Iframe",
            "Embed an external dashboard or status page",
            "browser",
            "embed",
            json!({ "embedUrl": "https://example.com", "allowScrolling": true }),
        ),
        W::Video => (
            "Video Stream",
            "Display an HLS or browser-playable video feed",
            "video",
            "embed",
            json!({ "feedUrl": "", "hasAutoPlay": false, "hasMutedAudio": true, "hasControls": true }),
        ),
        W::RssFeed => (
            "RSS Feed",
            "Subscribe to RSS 2.0 and Atom news feeds",
            "rss",
            "general",
            json!({ "feedUrls": ["https://blog.rust-lang.org/feed.xml"], "maximumAmountOfPosts": 8, "enableDangerousHtml": false }),
        ),
        W::StockPrice => (
            "Stock Price",
            "Track market price and daily delta for a ticker symbol",
            "chart-line",
            "finance",
            json!({ "stock": "NVDA", "timeRange": "1d" }),
        ),
        W::MinecraftServerStatus => (
            "Minecraft Server Status",
            "Shows online status, player count, MOTD, and version",
            "brand-minecraft",
            "gaming",
            json!({ "domain": "mc.hypixel.net:25565", "title": "Minecraft", "isBedrockServer": false }),
        ),
        W::DockerContainers => (
            "Docker Containers",
            "Monitor and control containers from configured Docker hosts",
            "brand-docker",
            "infrastructure",
            json!({ "showAll": true }),
        ),
        W::Timetable => (
            "Timetable",
            "Public transit departure schedule",
            "bus",
            "utilities",
            json!({ "station": "" }),
        ),
        W::CustomApi => (
            "Custom API",
            "Query a custom JSON endpoint or Custom Widget v2 definition",
            "api",
            "developer",
            json!({ "url": "", "customWidgetId": "" }),
        ),
        W::Assistant => (
            "AI Assistant",
            "Chat shortcut powered by your configured LLM assistant",
            "sparkles",
            "utilities",
            json!({ "title": "Homarr Assistant", "placeholder": "Ask anything..." }),
        ),
        W::Calendar => (
            "Calendar",
            "Upcoming releases and events from Sonarr, Radarr, and iCal",
            "calendar",
            "media",
            json!({ "useMilitaryTime": true }),
        ),
        W::Downloads => (
            "Download Client",
            "Monitor active torrent and Usenet downloads and speeds",
            "download",
            "media",
            json!({ "activeOnly": false, "columns": ["name", "progress", "speed"] }),
        ),
        W::MediaServer => (
            "Media Server",
            "Displays active playback sessions on Plex, Jellyfin, or Emby",
            "movie",
            "media",
            json!({ "showOnlyPlaying": false }),
        ),
        W::MediaRequestsRequestList => (
            "Media Requests",
            "View and approve/decline Overseerr and Jellyseerr requests",
            "list-check",
            "media",
            json!({ "linksTargetNewTab": true }),
        ),
        W::MediaRequestsRequestStats => (
            "Media Request Stats",
            "Summary counts of requested, pending, and available media",
            "chart-bar",
            "media",
            json!({}),
        ),
        W::MediaTranscoding => (
            "Media Transcoding",
            "Tdarr worker queue and storage saved summary",
            "cpu",
            "media",
            json!({}),
        ),
        W::MediaMissing => (
            "Missing Media",
            "Episodes and movies missing from your library",
            "file-search",
            "media",
            json!({}),
        ),
        W::MediaReleases => (
            "Media Releases",
            "Recent library releases and upcoming premieres",
            "disc",
            "media",
            json!({}),
        ),
        W::IndexerManager => (
            "Indexer Manager",
            "Monitor Prowlarr and Jackett indexer status",
            "search",
            "media",
            json!({}),
        ),
        W::DnsHoleSummary => (
            "DNS Hole Summary",
            "Blocked DNS queries and adblock percentage telemetry",
            "shield-check",
            "network",
            json!({}),
        ),
        W::DnsHoleControls => (
            "DNS Hole Controls",
            "Enable or disable Pi-hole / AdGuard Home protection",
            "toggle-right",
            "network",
            json!({ "showToggleAllButtons": true }),
        ),
        W::HealthMonitoring => (
            "Health Monitoring",
            "CPU, memory, disk, and node telemetry",
            "activity",
            "infrastructure",
            json!({ "fahrenheit": false }),
        ),
        W::SystemResources => (
            "System Resources",
            "Live CPU and RAM utilization gauges",
            "cpu",
            "infrastructure",
            json!({}),
        ),
        W::SystemDisks => (
            "System Disks",
            "Disk mount capacity and storage breakdown",
            "disc",
            "infrastructure",
            json!({}),
        ),
        W::SmartHomeEntityState => (
            "Smart Home Entity",
            "Display live state of a Home Assistant entity",
            "home-bolt",
            "smart-home",
            json!({ "entityId": "sensor.living_room_temperature" }),
        ),
        W::SmartHomeExecuteAutomation => (
            "Smart Home Automation",
            "Trigger a Home Assistant automation or script",
            "bolt",
            "smart-home",
            json!({ "entityId": "", "displayName": "Run Automation" }),
        ),
        W::SmartHomeControls => (
            "Smart Home Controls",
            "Toggle switches and lights in Home Assistant",
            "bulb",
            "smart-home",
            json!({ "entityIds": [] }),
        ),
        W::NetworkControllerSummary => (
            "Network Summary",
            "WAN/LAN health and client counts from your controller",
            "router",
            "network",
            json!({}),
        ),
        W::NetworkControllerStatus => (
            "Network Status",
            "Device up/down overview from UniFi / network controller",
            "network",
            "network",
            json!({}),
        ),
        W::Releases => (
            "Releases Tracker",
            "Follow latest GitHub, GitLab, Docker Hub, npm, or Codeberg releases",
            "tag",
            "developer",
            json!({ "provider": "github", "identifier": "homarr-labs/homarr" }),
        ),
        W::Firewall => (
            "Firewall",
            "OPNsense CPU, gateway, and interface summary",
            "wall",
            "network",
            json!({}),
        ),
        W::Notifications => (
            "Notifications",
            "Recent alerts from Gotify or Ntfy",
            "bell",
            "utilities",
            json!({}),
        ),
        W::Coolify => (
            "Coolify",
            "Application deployment status from Coolify",
            "server",
            "infrastructure",
            json!({}),
        ),
        W::ImmichServerStats => (
            "Immich Stats",
            "Photo, video, and storage counts from Immich",
            "photo",
            "media",
            json!({}),
        ),
        W::ImmichAlbumCarousel => (
            "Immich Album Carousel",
            "Preview shared photos from an Immich album",
            "album",
            "media",
            json!({ "albumId": "" }),
        ),
        W::PaperlessNgx => (
            "Paperless-ngx",
            "Document archive counts and inbox status",
            "file-text",
            "utilities",
            json!({}),
        ),
        W::ThingportLibraryStats => (
            "Thingport Library Stats",
            "3D model library totals and disk metrics",
            "cube",
            "media",
            json!({}),
        ),
        W::ThingportModels => (
            "Thingport Models",
            "Browse recent 3D models from Thingport",
            "box",
            "media",
            json!({}),
        ),
        W::ThingportFeaturedModel => (
            "Thingport Featured Model",
            "Showcase a featured 3D print model",
            "star",
            "media",
            json!({}),
        ),
        W::ThingportActivityHeatmap => (
            "Thingport Activity",
            "Print and upload activity heatmap",
            "flame",
            "media",
            json!({}),
        ),
        W::Patchmon => (
            "Patchmon",
            "OS package update status across servers",
            "package",
            "infrastructure",
            json!({}),
        ),
        W::Bazarr => (
            "Bazarr",
            "Missing subtitles and subtitle download queue",
            "subtitles",
            "media",
            json!({}),
        ),
        W::Tracearr => (
            "Tracearr",
            "Stream inspection metrics from Tracearr",
            "activity-heartbeat",
            "media",
            json!({}),
        ),
        W::SpeedtestTracker => (
            "Speedtest Tracker",
            "Latest download, upload, and ping speedtest result",
            "gauge",
            "network",
            json!({}),
        ),
        W::UptimeKuma => (
            "Uptime Kuma",
            "Monitor status summary from Uptime Kuma",
            "heartbeat",
            "infrastructure",
            json!({}),
        ),
        W::AudioStats => (
            "Audio Stats",
            "Music library and podcast statistics",
            "headphones",
            "media",
            json!({}),
        ),
        W::Umami => (
            "Umami Analytics",
            "Website visitors and pageviews summary",
            "chart-pie",
            "developer",
            json!({}),
        ),
        W::Vpn => (
            "VPN Status",
            "Public exit IP and tunnel state from Gluetun",
            "lock",
            "network",
            json!({}),
        ),
        W::ArchiveTeamWarrior => (
            "ArchiveTeam Warrior",
            "Active archival project progress and worker health",
            "archive",
            "utilities",
            json!({}),
        ),
        W::Ups => (
            "UPS Telemetry",
            "Battery charge, load percentage, and runtime from PeaNUT",
            "battery-charging",
            "infrastructure",
            json!({}),
        ),
        W::BeszelSystemTable => (
            "Beszel Systems Table",
            "Tabular view of monitored Beszel hosts",
            "table",
            "infrastructure",
            json!({}),
        ),
        W::BeszelSystemGrid => (
            "Beszel Systems Grid",
            "Card grid of monitored Beszel hosts",
            "layout-grid",
            "infrastructure",
            json!({}),
        ),
        W::BeszelAlerts => (
            "Beszel Alerts",
            "Active infrastructure alerts from Beszel",
            "alert-triangle",
            "infrastructure",
            json!({}),
        ),
        W::BeszelSystemStats => (
            "Beszel System Stats",
            "Single-host Beszel resource utilization",
            "cpu-2",
            "infrastructure",
            json!({}),
        ),
        W::Traefik => (
            "Traefik",
            "HTTP routers, services, and error rates from Traefik",
            "arrows-split-2",
            "network",
            json!({}),
        ),
        W::Stats => (
            "Integration Statistics",
            "Flexible metric cards from any Stats-compatible integration",
            "chart-dots",
            "infrastructure",
            json!({ "metrics": [] }),
        ),
        W::Wud => (
            "What's Up Docker",
            "Available container image updates",
            "refresh",
            "infrastructure",
            json!({}),
        ),
        W::Llamacpp => (
            "llama.cpp",
            "Inference server health and loaded model telemetry",
            "brain",
            "developer",
            json!({}),
        ),
        W::Wazuh => (
            "Wazuh SIEM",
            "Security agents and alert summary from Wazuh",
            "shield-lock",
            "infrastructure",
            json!({}),
        ),
    }
}
