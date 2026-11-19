//! Downloads & network group: download clients, DNS holes (Pi-hole, AdGuard Home, Technitium), firewall
//! (OPNsense), network controller (UniFi), VPN (Gluetun) and Traefik.

use async_trait::async_trait;
use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;

use super::{Handled, IntegrationGroup};
use crate::capabilities::dns_hole::{DnsHoleStatus, DnsHoleSummary};
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientJobsAndStatus,
    DownloadClientSelection,
};
use crate::capabilities::firewall::{
    FirewallCpuSummary, FirewallInterfacesSummary, FirewallMemorySummary, FirewallVersionSummary,
};
use crate::capabilities::network_controller::NetworkControllerSummary;
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::capabilities::traefik::TraefikDashboardData;
use crate::capabilities::vpn::VpnSummary;
use crate::runtime::IntegrationRuntime;
use crate::types::{
    DnsHoleSummary as LegacyDnsHoleSummary, DownloadClientItem as LegacyDownloadClientItem,
    DownloadClientQueue, IntegrationContext,
};

mod adguard;
mod aria2;
mod deluge;
mod gluetun;
mod http;
mod nzbget;
mod opnsense;
mod pihole;
mod qbittorrent;
mod sabnzbd;
mod selection;
mod slskd;
mod technitium;
mod traefik;
mod transmission;
mod unifi;

pub struct DownloadsNetworkGroup;

const KINDS: &[IntegrationKind] = &[
    IntegrationKind::SabNzbd,
    IntegrationKind::NzbGet,
    IntegrationKind::Deluge,
    IntegrationKind::Transmission,
    IntegrationKind::QBittorrent,
    IntegrationKind::Aria2,
    IntegrationKind::Slskd,
    IntegrationKind::PiHole,
    IntegrationKind::AdGuardHome,
    IntegrationKind::TechnitiumDns,
    IntegrationKind::Opnsense,
    IntegrationKind::UnifiController,
    IntegrationKind::Gluetun,
    IntegrationKind::Traefik,
];

/// Number of items the legacy downloads shape asks for.
const LEGACY_DOWNLOADS_LIMIT: usize = 100;

/// A download client (usenet or torrent), one implementation per kind.
#[async_trait]
trait DownloadClient: Send + Sync {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()>;

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus>;

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()>;

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()>;

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()>;

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()>;

    async fn delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> HomarrResult<()>;
}

/// A DNS blocker, one implementation per kind.
#[async_trait]
trait DnsHole: Send + Sync {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()>;

    async fn summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary>;

    async fn enable(&self, rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()>;

    /// `duration` in seconds; `None` disables until re-enabled.
    async fn disable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        duration: Option<u64>,
    ) -> HomarrResult<()>;
}

fn download_client(kind: IntegrationKind) -> Option<&'static dyn DownloadClient> {
    use IntegrationKind as K;
    Some(match kind {
        K::SabNzbd => &sabnzbd::Sabnzbd,
        K::NzbGet => &nzbget::NzbGet,
        K::Deluge => &deluge::Deluge,
        K::Transmission => &transmission::Transmission,
        K::QBittorrent => &qbittorrent::QBittorrent,
        K::Aria2 => &aria2::Aria2,
        K::Slskd => &slskd::Slskd,
        _ => return None,
    })
}

fn dns_hole(kind: IntegrationKind) -> Option<&'static dyn DnsHole> {
    use IntegrationKind as K;
    Some(match kind {
        K::PiHole => &pihole::PiHole,
        K::AdGuardHome => &adguard::AdGuardHome,
        K::TechnitiumDns => &technitium::Technitium,
        _ => return None,
    })
}

#[async_trait]
impl IntegrationGroup for DownloadsNetworkGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        if let Some(client) = download_client(ctx.kind) {
            return Some(client.test_connection(rt, ctx).await);
        }
        if let Some(hole) = dns_hole(ctx.kind) {
            return Some(hole.test_connection(rt, ctx).await);
        }
        match ctx.kind {
            IntegrationKind::Opnsense => Some(opnsense::test_connection(rt, ctx).await),
            IntegrationKind::UnifiController => Some(unifi::test_connection(rt, ctx).await),
            IntegrationKind::Gluetun => Some(gluetun::test_connection(rt, ctx).await),
            IntegrationKind::Traefik => Some(traefik::test_connection(rt, ctx).await),
            _ => None,
        }
    }

    // ---- dns hole ----------------------------------------------------------------------------

    async fn dns_hole_summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<DnsHoleSummary> {
        Some(dns_hole(ctx.kind)?.summary(rt, ctx).await)
    }

    async fn dns_hole_enable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(dns_hole(ctx.kind)?.enable(rt, ctx).await)
    }

    async fn dns_hole_disable(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        duration: Option<u64>,
    ) -> Handled<()> {
        Some(dns_hole(ctx.kind)?.disable(rt, ctx, duration).await)
    }

    // ---- downloads ---------------------------------------------------------------------------

    async fn downloads_jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        selection: Option<&DownloadClientSelection>,
    ) -> Handled<DownloadClientJobsAndStatus> {
        Some(
            download_client(ctx.kind)?
                .jobs_and_status(rt, ctx, limit, selection)
                .await,
        )
    }

    async fn downloads_pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(download_client(ctx.kind)?.pause_queue(rt, ctx).await)
    }

    async fn downloads_resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(download_client(ctx.kind)?.resume_queue(rt, ctx).await)
    }

    async fn downloads_pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> Handled<()> {
        Some(download_client(ctx.kind)?.pause_item(rt, ctx, item).await)
    }

    async fn downloads_resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> Handled<()> {
        Some(download_client(ctx.kind)?.resume_item(rt, ctx, item).await)
    }

    async fn downloads_delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> Handled<()> {
        Some(
            download_client(ctx.kind)?
                .delete_item(rt, ctx, item, from_disk)
                .await,
        )
    }

    // ---- firewall (OPNsense) -----------------------------------------------------------------

    async fn firewall_cpu(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<FirewallCpuSummary> {
        (ctx.kind == IntegrationKind::Opnsense).then_some(())?;
        Some(opnsense::cpu(rt, ctx).await)
    }

    async fn firewall_memory(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<FirewallMemorySummary> {
        (ctx.kind == IntegrationKind::Opnsense).then_some(())?;
        Some(opnsense::memory(rt, ctx).await)
    }

    async fn firewall_interfaces(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<FirewallInterfacesSummary>> {
        (ctx.kind == IntegrationKind::Opnsense).then_some(())?;
        Some(opnsense::interfaces(rt, ctx).await)
    }

    async fn firewall_version(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<FirewallVersionSummary> {
        (ctx.kind == IntegrationKind::Opnsense).then_some(())?;
        Some(opnsense::version(rt, ctx).await)
    }

    // ---- network controller (UniFi), VPN (Gluetun), Traefik ----------------------------------

    async fn network_summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<NetworkControllerSummary> {
        (ctx.kind == IntegrationKind::UnifiController).then_some(())?;
        Some(unifi::network_summary(rt, ctx).await)
    }

    async fn vpn_summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<VpnSummary> {
        (ctx.kind == IntegrationKind::Gluetun).then_some(())?;
        Some(gluetun::summary(rt, ctx).await)
    }

    async fn traefik_dashboard(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<TraefikDashboardData> {
        (ctx.kind == IntegrationKind::Traefik).then_some(())?;
        Some(traefik::dashboard(rt, ctx).await)
    }

    // ---- stats (`existing.ts`) ----------------------------------------------------------------

    fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        use super::{count_metric, stat_metric};
        use IntegrationKind as K;
        match kind {
            K::SabNzbd
            | K::NzbGet
            | K::QBittorrent
            | K::Deluge
            | K::Transmission
            | K::Slskd
            | K::Aria2 => vec![
                stat_metric("download", "Download", StatsUnit::BytesPerSecond),
                stat_metric("upload", "Upload", StatsUnit::BytesPerSecond),
                stat_metric("paused", "Paused", StatsUnit::Text),
            ],
            K::PiHole | K::AdGuardHome | K::TechnitiumDns => vec![
                count_metric("dnsQueriesToday", "Queries today"),
                count_metric("adsBlockedToday", "Blocked today"),
                stat_metric("adsBlockedTodayPercentage", "Blocked", StatsUnit::Percent),
                count_metric("domainsBeingBlocked", "Blocked domains"),
            ],
            K::Traefik => vec![
                count_metric("routers", "Routers"),
                count_metric("services", "Services"),
                count_metric("middlewares", "Middleware"),
            ],
            K::Gluetun => vec![
                stat_metric("status", "VPN status", StatsUnit::Text),
                stat_metric("publicIp", "Public IP", StatsUnit::Text),
                stat_metric("country", "Country", StatsUnit::Text),
            ],
            K::UnifiController => vec![
                count_metric("wifi", "Wi-Fi clients"),
                count_metric("lan", "Wired clients"),
                stat_metric("latency", "WAN latency", StatsUnit::Milliseconds),
            ],
            K::Opnsense => vec![stat_metric("cpu", "CPU", StatsUnit::Percent)],
            _ => Vec::new(),
        }
    }

    async fn stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        if let Some(client) = download_client(ctx.kind) {
            return Some(client.jobs_and_status(rt, ctx, 1, None).await.map(|data| {
                StatsValues::from([
                    (
                        "download".into(),
                        StatsValue::Number(data.status.rates.down),
                    ),
                    (
                        "upload".into(),
                        data.status
                            .rates
                            .up
                            .map_or(StatsValue::Null, StatsValue::Number),
                    ),
                    ("paused".into(), StatsValue::Bool(data.status.paused)),
                ])
            }));
        }
        if let Some(hole) = dns_hole(ctx.kind) {
            return Some(hole.summary(rt, ctx).await.map(|summary| {
                StatsValues::from([
                    (
                        "dnsQueriesToday".into(),
                        StatsValue::Number(summary.dns_queries_today as f64),
                    ),
                    (
                        "adsBlockedToday".into(),
                        StatsValue::Number(summary.ads_blocked_today as f64),
                    ),
                    (
                        "adsBlockedTodayPercentage".into(),
                        StatsValue::Number(summary.ads_blocked_today_percentage),
                    ),
                    (
                        "domainsBeingBlocked".into(),
                        StatsValue::Number(summary.domains_being_blocked as f64),
                    ),
                ])
            }));
        }
        Some(match ctx.kind {
            IntegrationKind::Traefik => traefik::dashboard(rt, ctx).await.map(|data| {
                StatsValues::from([
                    (
                        "routers".into(),
                        StatsValue::Number(
                            (data.http.routers.total
                                + data.tcp.routers.total
                                + data.udp.routers.total) as f64,
                        ),
                    ),
                    (
                        "services".into(),
                        StatsValue::Number(
                            (data.http.services.total
                                + data.tcp.services.total
                                + data.udp.services.total) as f64,
                        ),
                    ),
                    (
                        "middlewares".into(),
                        StatsValue::Number(
                            (data.http.middlewares.total + data.tcp.middlewares.total) as f64,
                        ),
                    ),
                ])
            }),
            IntegrationKind::Gluetun => gluetun::summary(rt, ctx).await.map(|data| {
                StatsValues::from([
                    ("status".into(), StatsValue::Text(data.vpn_status)),
                    ("publicIp".into(), StatsValue::Text(data.public_ip)),
                    (
                        "country".into(),
                        data.country.map_or(StatsValue::Null, StatsValue::Text),
                    ),
                ])
            }),
            IntegrationKind::UnifiController => unifi::network_summary(rt, ctx).await.map(|data| {
                StatsValues::from([
                    (
                        "wifi".into(),
                        StatsValue::Number((data.wifi.users + data.wifi.guests) as f64),
                    ),
                    (
                        "lan".into(),
                        StatsValue::Number((data.lan.users + data.lan.guests) as f64),
                    ),
                    ("latency".into(), StatsValue::Number(data.www.latency)),
                ])
            }),
            IntegrationKind::Opnsense => opnsense::cpu(rt, ctx)
                .await
                .map(|data| StatsValues::from([("cpu".into(), StatsValue::Number(data.total))])),
            _ => return None,
        })
    }

    // ---- legacy adapters (old `crate::types` shapes on top of the ported implementations) ----

    async fn legacy_dns_hole_summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<LegacyDnsHoleSummary> {
        let summary = dns_hole(ctx.kind)?.summary(rt, ctx).await;
        Some(summary.map(to_legacy_dns_summary))
    }

    async fn legacy_dns_hole_set_enabled(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        enabled: bool,
    ) -> Handled<()> {
        let hole = dns_hole(ctx.kind)?;
        Some(if enabled {
            hole.enable(rt, ctx).await
        } else {
            hole.disable(rt, ctx, None).await
        })
    }

    async fn legacy_downloads(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<DownloadClientQueue> {
        let client = download_client(ctx.kind)?;
        let result = client
            .jobs_and_status(rt, ctx, LEGACY_DOWNLOADS_LIMIT, None)
            .await;
        Some(result.map(to_legacy_queue))
    }
}

fn to_legacy_dns_summary(summary: DnsHoleSummary) -> LegacyDnsHoleSummary {
    LegacyDnsHoleSummary {
        status: match summary.status {
            Some(DnsHoleStatus::Disabled) => "disabled",
            Some(DnsHoleStatus::Enabled) | None => "enabled",
        }
        .to_string(),
        domains_being_blocked: summary.domains_being_blocked,
        ads_blocked_today: summary.ads_blocked_today,
        ads_percentage_today: summary.ads_blocked_today_percentage,
        dns_queries_today: summary.dns_queries_today,
    }
}

fn bytes(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value as u64
    } else {
        0
    }
}

fn to_legacy_item(item: DownloadClientItem) -> LegacyDownloadClientItem {
    let state = serde_json::to_value(item.state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let category = match item.category {
        Some(DownloadClientCategory::One(category)) => Some(category),
        Some(DownloadClientCategory::Many(categories)) => Some(categories.join(", ")),
        None => None,
    }
    .filter(|category| !category.is_empty());
    LegacyDownloadClientItem {
        id: item.id,
        name: item.name,
        state,
        size_bytes: bytes(item.size),
        progress: item.progress,
        download_speed_bps: bytes(item.down_speed.unwrap_or(0.0)),
        upload_speed_bps: bytes(item.up_speed.unwrap_or(0.0)),
        // Positive `time` is the ETA in milliseconds; 0 means unknown/infinite, negative means finished.
        eta_seconds: (item.time > 0.0).then(|| bytes(item.time / 1000.0)),
        category,
    }
}

fn to_legacy_queue(data: DownloadClientJobsAndStatus) -> DownloadClientQueue {
    DownloadClientQueue {
        download_speed_bps: bytes(data.status.rates.down),
        upload_speed_bps: bytes(data.status.rates.up.unwrap_or(0.0)),
        paused: data.status.paused,
        items: data.items.into_iter().map(to_legacy_item).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::downloads::{DownloadClientItemState, DownloadClientType};

    #[test]
    fn every_kind_has_an_implementation() {
        for kind in KINDS {
            let known = download_client(*kind).is_some()
                || dns_hole(*kind).is_some()
                || matches!(
                    kind,
                    IntegrationKind::Opnsense
                        | IntegrationKind::UnifiController
                        | IntegrationKind::Gluetun
                        | IntegrationKind::Traefik
                );
            assert!(known, "{} has no implementation", kind.as_str());
        }
    }

    #[test]
    fn adapts_items_to_the_legacy_shape() {
        let item = DownloadClientItem {
            id: "1".into(),
            index: 0.0,
            name: "n".into(),
            item_type: DownloadClientType::Torrent,
            size: 100.5,
            sent: None,
            received: None,
            down_speed: Some(10.0),
            up_speed: None,
            time: 5_500.0,
            added: None,
            state: DownloadClientItemState::Leeching,
            progress: 0.5,
            category: Some(DownloadClientCategory::Many(vec!["a".into(), "b".into()])),
        };
        let legacy = to_legacy_item(item);
        assert_eq!(legacy.state, "leeching");
        assert_eq!(legacy.size_bytes, 100);
        assert_eq!(legacy.eta_seconds, Some(5));
        assert_eq!(legacy.category.as_deref(), Some("a, b"));
        assert_eq!(legacy.upload_speed_bps, 0);
    }
}
