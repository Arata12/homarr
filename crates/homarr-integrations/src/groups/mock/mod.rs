//! Mock group: the `Mock` kind, backed by static fixture data.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use homarr_definitions::IntegrationKind;

use super::{Handled, IntegrationGroup};
use crate::capabilities::dns_hole::{DnsHoleStatus, DnsHoleSummary as CapDnsHoleSummary};
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem as CapDownloadClientItem, DownloadClientItemState,
    DownloadClientJobsAndStatus, DownloadClientRates, DownloadClientSelection,
    DownloadClientStatus, DownloadClientType,
};
use crate::capabilities::firewall::{
    FirewallCpuSummary, FirewallInterface, FirewallInterfacesSummary, FirewallMemorySummary,
    FirewallVersionSummary,
};
use crate::capabilities::media_releases::{MediaRelease, MediaReleaseImageUrls, MediaType};
use crate::capabilities::network_controller::{
    EnabledStatus, NetworkControllerSummary, UsersSummary, VpnUsersSummary, WwwSummary,
};
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::capabilities::traefik::{
    TraefikDashboardData, TraefikProtocolSummary, TraefikResourceSummary, TraefikUdpSummary,
};
use crate::capabilities::vpn::{VpnProviderDetails, VpnSummary};
use crate::runtime::IntegrationRuntime;
use crate::types::{
    CalendarEvent, DnsHoleSummary, DownloadClientQueue, HealthMonitoringInfo, IndexerInfo,
    IntegrationContext, MediaOrganizerItem, MediaRequestItem, MediaRequestStats,
    MediaServerSession, MediaTranscodingSummary, NotificationItem, SmartHomeEntity, StatsResponse,
    UpsSummary,
};

pub(crate) mod legacy;

pub struct MockGroup;

const KINDS: &[IntegrationKind] = &[IntegrationKind::Mock];

#[async_trait]
impl IntegrationGroup for MockGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    async fn test_connection(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(Ok(()))
    }

    async fn legacy_calendar_events(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> Handled<Vec<CalendarEvent>> {
        Some(Ok(legacy::calendar_events()))
    }

    async fn legacy_dns_hole_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<DnsHoleSummary> {
        Some(Ok(legacy::dns_hole_summary()))
    }

    async fn legacy_dns_hole_set_enabled(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _enabled: bool,
    ) -> Handled<()> {
        Some(Ok(()))
    }

    async fn legacy_downloads(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<DownloadClientQueue> {
        Some(Ok(legacy::downloads()))
    }

    async fn legacy_media_sessions(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaServerSession>> {
        Some(Ok(legacy::media_sessions()))
    }

    async fn legacy_media_requests(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<(Vec<MediaRequestItem>, MediaRequestStats)> {
        Some(Ok(legacy::media_requests()))
    }

    async fn legacy_media_request_action(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _request_id: &str,
        _approve: bool,
    ) -> Handled<()> {
        Some(Ok(()))
    }

    async fn legacy_health_monitoring(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<HealthMonitoringInfo> {
        Some(Ok(legacy::health_monitoring()))
    }

    async fn legacy_smart_home_entities(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<SmartHomeEntity>> {
        Some(Ok(legacy::smart_home_entities()))
    }

    async fn legacy_smart_home_action(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _entity_id: &str,
        _service: &str,
    ) -> Handled<()> {
        Some(Ok(()))
    }

    async fn legacy_notifications(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<NotificationItem>> {
        Some(Ok(legacy::notifications()))
    }

    async fn legacy_indexers(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<IndexerInfo>> {
        Some(Ok(legacy::indexers()))
    }

    async fn legacy_media_missing(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaOrganizerItem>> {
        Some(Ok(legacy::media_missing()))
    }

    async fn legacy_media_transcoding(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<MediaTranscodingSummary> {
        Some(Ok(legacy::media_transcoding()))
    }

    async fn legacy_ups_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<UpsSummary>> {
        Some(Ok(legacy::ups_summary()))
    }

    async fn legacy_service_version(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Option<String>> {
        Some(Ok(Some("3.0.0-mock".into())))
    }

    async fn legacy_stats(
        &self,
        _rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsResponse> {
        Some(Ok(legacy::stats(&ctx.name)))
    }

    fn stats_metrics(&self, _kind: IntegrationKind) -> Vec<StatsMetric> {
        use super::{count_metric, stat_metric};
        vec![
            count_metric("documents", "Documents"),
            count_metric("songs", "Songs"),
            stat_metric("storage", "Storage", StatsUnit::Bytes),
        ]
    }

    async fn stats(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        Some(Ok(StatsValues::from([
            ("documents".into(), StatsValue::Number(1234.0)),
            ("songs".into(), StatsValue::Number(5678.0)),
            ("storage".into(), StatsValue::Number(12_345_678_900.0)),
        ])))
    }

    async fn dns_hole_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<CapDnsHoleSummary> {
        let legacy = legacy::dns_hole_summary();
        Some(Ok(CapDnsHoleSummary {
            status: Some(if legacy.status == "disabled" {
                DnsHoleStatus::Disabled
            } else {
                DnsHoleStatus::Enabled
            }),
            domains_being_blocked: legacy.domains_being_blocked,
            ads_blocked_today: legacy.ads_blocked_today,
            ads_blocked_today_percentage: legacy.ads_percentage_today,
            dns_queries_today: legacy.dns_queries_today,
        }))
    }

    async fn downloads_jobs_and_status(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> Handled<DownloadClientJobsAndStatus> {
        let q = legacy::downloads();
        Some(Ok(DownloadClientJobsAndStatus {
            status: DownloadClientStatus {
                paused: q.paused,
                rates: DownloadClientRates {
                    down: q.download_speed_bps as f64,
                    up: Some(q.upload_speed_bps as f64),
                },
                types: vec![DownloadClientType::Torrent],
            },
            items: q
                .items
                .into_iter()
                .take(limit)
                .enumerate()
                .map(|(idx, item)| CapDownloadClientItem {
                    id: item.id,
                    index: idx as f64,
                    name: item.name,
                    item_type: DownloadClientType::Torrent,
                    size: item.size_bytes as f64,
                    sent: None,
                    received: None,
                    down_speed: Some(item.download_speed_bps as f64),
                    up_speed: Some(item.upload_speed_bps as f64),
                    time: item.eta_seconds.map_or(0.0, |s| (s * 1000) as f64),
                    added: None,
                    state: match item.state.as_str() {
                        "seeding" => DownloadClientItemState::Seeding,
                        "paused" => DownloadClientItemState::Paused,
                        _ => DownloadClientItemState::Downloading,
                    },
                    progress: item.progress,
                    category: item.category.map(DownloadClientCategory::One),
                })
                .collect(),
            ratio_items: None,
        }))
    }

    async fn media_releases(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaRelease>> {
        let now = Utc::now();
        Some(Ok(vec![
            MediaRelease {
                id: "mock-rel-1".into(),
                media_type: MediaType::Tv,
                title: "Severance".into(),
                subtitle: Some("S02E08 \u{2013} Sweet Vitriol".into()),
                description: Some("Lumon employees uncover more about the severed floor.".into()),
                release_date: now - Duration::hours(6),
                release_date_is_date_only: Some(false),
                image_urls: MediaReleaseImageUrls {
                    poster: None,
                    backdrop: None,
                },
                producer: Some("Apple TV+".into()),
                price: None,
                rating: Some("8.9".into()),
                tags: vec!["Sci-Fi".into(), "Thriller".into()],
                href: "#".into(),
                length: Some(3240.0),
            },
            MediaRelease {
                id: "mock-rel-2".into(),
                media_type: MediaType::Movie,
                title: "Dune: Part Two".into(),
                subtitle: Some("2024".into()),
                description: None,
                release_date: now - Duration::days(2),
                release_date_is_date_only: Some(true),
                image_urls: MediaReleaseImageUrls {
                    poster: None,
                    backdrop: None,
                },
                producer: Some("Legendary".into()),
                price: None,
                rating: Some("8.5".into()),
                tags: vec!["Sci-Fi".into()],
                href: "#".into(),
                length: Some(9960.0),
            },
        ]))
    }

    async fn firewall_cpu(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallCpuSummary> {
        Some(Ok(FirewallCpuSummary { total: 14.2 }))
    }

    async fn firewall_memory(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallMemorySummary> {
        Some(Ok(FirewallMemorySummary {
            used: 3_080_000_000.0,
            total: 8_000_000_000.0,
            percent: 38.5,
        }))
    }

    async fn firewall_interfaces(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<FirewallInterfacesSummary>> {
        Some(Ok(vec![FirewallInterfacesSummary {
            data: vec![
                FirewallInterface {
                    name: "WAN".into(),
                    receive: 12_400_000.0,
                    transmit: 3_200_000.0,
                },
                FirewallInterface {
                    name: "LAN".into(),
                    receive: 48_100_000.0,
                    transmit: 92_300_000.0,
                },
            ],
            timestamp: Utc::now(),
        }]))
    }

    async fn firewall_version(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallVersionSummary> {
        Some(Ok(FirewallVersionSummary {
            version: "OPNsense 25.1".into(),
        }))
    }

    async fn vpn_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<VpnSummary> {
        Some(Ok(VpnSummary {
            vpn_status: "running".into(),
            dns_status: Some("running".into()),
            public_ip: "198.51.100.42".into(),
            country: Some("Switzerland".into()),
            city: Some("Zurich".into()),
            vpn_provider: Some(VpnProviderDetails {
                protocol: "wireguard".into(),
                provider: "Mullvad".into(),
            }),
        }))
    }

    async fn traefik_dashboard(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<TraefikDashboardData> {
        let mk = |total, enabled| TraefikResourceSummary {
            total,
            enabled,
            warnings: 0,
            errors: 0,
        };
        Some(Ok(TraefikDashboardData {
            version: Some("3.3.2".into()),
            entry_points: vec!["web".into(), "websecure".into()],
            resources: Vec::new(),
            failed_endpoints: Vec::new(),
            http: TraefikProtocolSummary {
                routers: mk(20, 20),
                services: mk(18, 18),
                middlewares: mk(11, 11),
            },
            tcp: TraefikProtocolSummary {
                routers: mk(4, 4),
                services: mk(4, 4),
                middlewares: mk(0, 0),
            },
            udp: TraefikUdpSummary {
                routers: mk(0, 0),
                services: mk(0, 0),
            },
        }))
    }

    async fn network_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<NetworkControllerSummary> {
        Some(Ok(NetworkControllerSummary {
            wan_status: EnabledStatus::Enabled,
            www: WwwSummary {
                status: EnabledStatus::Enabled,
                latency: 8.4,
                ping: 8.4,
                uptime: 99.98,
            },
            wifi: UsersSummary {
                status: EnabledStatus::Enabled,
                users: 22,
                guests: 2,
            },
            lan: UsersSummary {
                status: EnabledStatus::Enabled,
                users: 10,
                guests: 0,
            },
            vpn: VpnUsersSummary {
                status: EnabledStatus::Enabled,
                users: Some(1),
            },
        }))
    }

    async fn integration_data(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        operation: &str,
        _input: &serde_json::Value,
    ) -> Handled<serde_json::Value> {
        Some(Ok(legacy::generic_widget_data(operation)))
    }
}
