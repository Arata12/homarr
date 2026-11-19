use chrono::{Duration, Utc};
use serde_json::{json, Value};

use crate::types::{
    CalendarEvent, ClusterNodeInfo, DnsHoleSummary, DownloadClientItem, DownloadClientQueue,
    HealthMonitoringDisk, HealthMonitoringInfo, IndexerInfo, MediaOrganizerItem, MediaRequestItem,
    MediaRequestStats, MediaServerSession, MediaTranscodingSummary, NotificationItem,
    SmartHomeEntity, StatMetric, StatsResponse, UpsSummary,
};

pub fn calendar_events() -> Vec<CalendarEvent> {
    let now = Utc::now();
    vec![
        CalendarEvent {
            id: "mock-cal-1".into(),
            title: "Severance S02E08".into(),
            sub_title: Some("The After Hours".into()),
            description: Some("Upcoming episode in Sonarr".into()),
            start_date: (now + Duration::hours(4)).to_rfc3339(),
            end_date: Some((now + Duration::hours(5)).to_rfc3339()),
            image_url: None,
            link: None,
            indicator_color: "#3b82f6".into(),
        },
        CalendarEvent {
            id: "mock-cal-2".into(),
            title: "Dune: Part Two (4K BluRay)".into(),
            sub_title: Some("Radarr Release".into()),
            description: Some("Digital / Physical release".into()),
            start_date: (now + Duration::days(2)).to_rfc3339(),
            end_date: None,
            image_url: None,
            link: None,
            indicator_color: "#f59e0b".into(),
        },
        CalendarEvent {
            id: "mock-cal-3".into(),
            title: "Homelab Backup Window".into(),
            sub_title: Some("Maintenance".into()),
            description: Some("Scheduled weekly snapshot".into()),
            start_date: (now + Duration::days(4)).to_rfc3339(),
            end_date: None,
            image_url: None,
            link: None,
            indicator_color: "#10b981".into(),
        },
    ]
}

pub fn dns_hole_summary() -> DnsHoleSummary {
    DnsHoleSummary {
        status: "enabled".into(),
        domains_being_blocked: 184_320,
        ads_blocked_today: 12_487,
        ads_percentage_today: 21.4,
        dns_queries_today: 58_350,
    }
}

pub fn downloads() -> DownloadClientQueue {
    DownloadClientQueue {
        download_speed_bps: 48_500_000,
        upload_speed_bps: 6_200_000,
        paused: false,
        items: vec![
            DownloadClientItem {
                id: "mock-dl-1".into(),
                name: "ubuntu-24.04.2-live-server-amd64.iso".into(),
                state: "downloading".into(),
                size_bytes: 2_845_000_000,
                progress: 0.74,
                download_speed_bps: 36_000_000,
                upload_speed_bps: 2_100_000,
                eta_seconds: Some(21),
                category: Some("linux".into()),
            },
            DownloadClientItem {
                id: "mock-dl-2".into(),
                name: "debian-12.9.0-amd64-netinst.iso".into(),
                state: "seeding".into(),
                size_bytes: 660_000_000,
                progress: 1.0,
                download_speed_bps: 0,
                upload_speed_bps: 4_100_000,
                eta_seconds: None,
                category: Some("linux".into()),
            },
            DownloadClientItem {
                id: "mock-dl-3".into(),
                name: "homelab-backup-bundle.tar.zst".into(),
                state: "downloading".into(),
                size_bytes: 1_240_000_000,
                progress: 0.42,
                download_speed_bps: 12_500_000,
                upload_speed_bps: 0,
                eta_seconds: Some(58),
                category: Some("archives".into()),
            },
        ],
    }
}

pub fn media_sessions() -> Vec<MediaServerSession> {
    vec![
        MediaServerSession {
            id: "mock-sess-1".into(),
            username: "alex".into(),
            item_name: "S01E03 - Long, Long Time".into(),
            series_name: Some("The Last of Us".into()),
            client_name: "Apple TV 4K".into(),
            is_paused: false,
            progress_percent: 62.5,
        },
        MediaServerSession {
            id: "mock-sess-2".into(),
            username: "maria".into(),
            item_name: "Interstellar (2014)".into(),
            series_name: None,
            client_name: "Jellyfin Web".into(),
            is_paused: true,
            progress_percent: 38.0,
        },
    ]
}

pub fn media_requests() -> (Vec<MediaRequestItem>, MediaRequestStats) {
    let items = vec![
        MediaRequestItem {
            id: "101".into(),
            title: "Andor".into(),
            media_type: "tv".into(),
            status: "pending".into(),
            requested_by: "alex".into(),
            created_at: Utc::now().to_rfc3339(),
            poster_url: None,
        },
        MediaRequestItem {
            id: "102".into(),
            title: "Blade Runner 2049".into(),
            media_type: "movie".into(),
            status: "available".into(),
            requested_by: "maria".into(),
            created_at: (Utc::now() - Duration::hours(5)).to_rfc3339(),
            poster_url: None,
        },
    ];
    let stats = MediaRequestStats {
        total: 42,
        pending: 3,
        approved: 9,
        available: 29,
        declined: 1,
    };
    (items, stats)
}

pub fn health_monitoring() -> HealthMonitoringInfo {
    HealthMonitoringInfo {
        system_name: "homelab-node-01".into(),
        version: "6.8.12-4-pve".into(),
        cpu_percent: 28.4,
        cpu_temp_c: Some(46.5),
        memory_used_bytes: 19_327_352_832,
        memory_total_bytes: 34_359_738_368,
        uptime_seconds: 1_248_920,
        load_average: Some([0.84, 0.72, 0.65]),
        disks: vec![
            HealthMonitoringDisk {
                name: "nvme0n1".into(),
                mount_point: "/".into(),
                used_bytes: 185_000_000_000,
                total_bytes: 500_000_000_000,
            },
            HealthMonitoringDisk {
                name: "zfs-pool".into(),
                mount_point: "/mnt/tank".into(),
                used_bytes: 4_200_000_000_000,
                total_bytes: 8_000_000_000_000,
            },
        ],
        nodes: vec![
            ClusterNodeInfo {
                id: "node/pve1".into(),
                name: "pve1".into(),
                status: "online".into(),
                cpu_percent: 24.0,
                memory_percent: 56.2,
                uptime_seconds: 1_248_920,
            },
            ClusterNodeInfo {
                id: "node/pve2".into(),
                name: "pve2".into(),
                status: "online".into(),
                cpu_percent: 18.7,
                memory_percent: 41.9,
                uptime_seconds: 980_410,
            },
        ],
    }
}

pub fn smart_home_entities() -> Vec<SmartHomeEntity> {
    vec![
        SmartHomeEntity {
            entity_id: "light.office_desk".into(),
            friendly_name: "Office Desk Lamp".into(),
            state: "on".into(),
            unit_of_measurement: None,
            domain: "light".into(),
        },
        SmartHomeEntity {
            entity_id: "sensor.living_room_temperature".into(),
            friendly_name: "Living Room Temp".into(),
            state: "21.8".into(),
            unit_of_measurement: Some("°C".into()),
            domain: "sensor".into(),
        },
        SmartHomeEntity {
            entity_id: "switch.server_rack_fan".into(),
            friendly_name: "Server Rack Fan".into(),
            state: "off".into(),
            unit_of_measurement: None,
            domain: "switch".into(),
        },
    ]
}

pub fn notifications() -> Vec<NotificationItem> {
    vec![
        NotificationItem {
            id: "mock-notif-1".into(),
            title: "Backup Completed".into(),
            body: "Daily snapshot verified cleanly (4.2 GB)".into(),
            priority: "normal".into(),
            created_at: Utc::now().to_rfc3339(),
        },
        NotificationItem {
            id: "mock-notif-2".into(),
            title: "Certificate Renewed".into(),
            body: "TLS cert for *.homelab.local renewed for 90 days".into(),
            priority: "low".into(),
            created_at: (Utc::now() - Duration::hours(3)).to_rfc3339(),
        },
    ]
}

pub fn indexers() -> Vec<IndexerInfo> {
    vec![
        IndexerInfo {
            id: "1".into(),
            name: "Ubuntu Torrents".into(),
            enabled: true,
            status: "healthy".into(),
            protocol: "torrent".into(),
        },
        IndexerInfo {
            id: "2".into(),
            name: "Usenet Indexer".into(),
            enabled: true,
            status: "healthy".into(),
            protocol: "usenet".into(),
        },
    ]
}

pub fn media_transcoding() -> MediaTranscodingSummary {
    MediaTranscodingSummary {
        total_files: 1_840,
        queue_count: 14,
        space_saved_bytes: 392_000_000_000,
        active_workers: 2,
    }
}

pub fn media_missing() -> Vec<MediaOrganizerItem> {
    vec![
        MediaOrganizerItem {
            id: "m1".into(),
            title: "Severance - S02E09".into(),
            subtitle: Some("Missing Episode".into()),
            release_date: Some(Utc::now().to_rfc3339()),
        },
        MediaOrganizerItem {
            id: "m2".into(),
            title: "The Batman Part II".into(),
            subtitle: Some("Announced".into()),
            release_date: None,
        },
    ]
}

pub fn ups_summary() -> Vec<UpsSummary> {
    vec![UpsSummary {
        name: "APC Back-UPS 1500".into(),
        status: "OL".into(),
        battery_charge_percent: 100.0,
        load_percent: 24.0,
        runtime_seconds: 2_340,
    }]
}

pub fn stats(name: &str) -> StatsResponse {
    StatsResponse {
        integration_name: name.to_string(),
        kind: "mock".to_string(),
        metrics: vec![
            StatMetric {
                label: "Active Items".into(),
                value: "128".into(),
                unit: None,
            },
            StatMetric {
                label: "Uptime".into(),
                value: "99.98".into(),
                unit: Some("%".into()),
            },
            StatMetric {
                label: "Latency".into(),
                value: "4.2".into(),
                unit: Some("ms".into()),
            },
        ],
    }
}

pub fn generic_widget_data(kind: &str) -> Value {
    match kind {
        "firewall" => json!({
            "version": "OPNsense 25.1",
            "cpuPercent": 14.2,
            "memoryPercent": 38.5,
            "interfaces": [
                { "name": "WAN", "status": "up", "rxBytes": 12_400_000_000u64, "txBytes": 3_200_000_000u64 },
                { "name": "LAN", "status": "up", "rxBytes": 48_100_000_000u64, "txBytes": 92_300_000_000u64 }
            ]
        }),
        "networkControllerSummary" | "networkControllerStatus" => json!({
            "status": "online",
            "wanStatus": "connected",
            "clientsConnected": 34,
            "devicesOnline": 6,
            "downloadBps": 14_500_000,
            "uploadBps": 3_200_000
        }),
        "uptimeKuma" => json!({
            "monitors": [
                { "name": "Homarr Dashboard", "status": "up", "uptime24h": 100.0, "pingMs": 3 },
                { "name": "Reverse Proxy", "status": "up", "uptime24h": 99.95, "pingMs": 7 },
                { "name": "Media Server", "status": "up", "uptime24h": 99.8, "pingMs": 12 }
            ]
        }),
        "vpn" => json!({
            "publicIp": "198.51.100.42",
            "country": "Switzerland",
            "status": "connected",
            "provider": "WireGuard"
        }),
        "traefik" => json!({
            "routersTotal": 24,
            "routersErrors": 0,
            "servicesTotal": 22,
            "middlewaresTotal": 11
        }),
        "coolify" => json!({
            "applications": [
                { "name": "homarr-prod", "status": "running" },
                { "name": "vaultwarden", "status": "running" }
            ],
            "servers": 2
        }),
        "beszelSystemTable" | "beszelSystemGrid" | "beszelSystemStats" | "beszelAlerts" => json!({
            "systems": [
                { "name": "proxmox-01", "status": "up", "cpu": 23.4, "mem": 61.0, "disk": 48.2 },
                { "name": "nas-01", "status": "up", "cpu": 11.2, "mem": 44.5, "disk": 72.8 }
            ],
            "alerts": []
        }),
        "immich-serverStats" | "immich-albumCarousel" => json!({
            "photos": 18_420,
            "videos": 1_240,
            "usageBytes": 248_000_000_000u64,
            "albums": []
        }),
        "paperlessNgx" => json!({
            "documentsTotal": 1_482,
            "documentsInbox": 7,
            "charactersCount": 4_210_980
        }),
        "bazarr" => json!({ "moviesMissing": 4, "episodesMissing": 11 }),
        "speedtestTracker" => json!({
            "downloadMbps": 942.5,
            "uploadMbps": 891.2,
            "pingMs": 4.1,
            "serverName": "Zurich Fiber"
        }),
        "umami" => {
            json!({ "pageviews": 4_812, "visitors": 1_290, "bounces": 412, "totaltime": 84_200 })
        }
        "wud" => json!({ "containers": 18, "updatesAvailable": 2 }),
        "llamacpp" => {
            json!({ "status": "ok", "model": "qwen2.5-coder-14b", "slotsIdle": 4, "slotsProcessing": 0 })
        }
        "wazuh" => json!({ "activeAgents": 12, "disconnectedAgents": 0, "alerts24h": 37 }),
        _ => json!({ "status": "ok", "mock": true }),
    }
}
