//! Integration groups.
//!
//! A group owns a set of `IntegrationKind`s (each kind belongs to exactly one group, see the test below) and
//! implements the capabilities those kinds support by overriding the matching `IntegrationGroup` methods.
//! A method returns `None` when the group does not handle `ctx.kind` for that capability, so
//! `IntegrationRegistry` can answer "does not support" instead of returning empty data.
#![allow(dead_code)] // Capability methods are wired to consumers in later phases.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;
use tokio::sync::{mpsc, watch};

pub(crate) fn stat_metric(
    key: &str,
    label: &str,
    unit: crate::capabilities::stats::StatsUnit,
) -> crate::capabilities::stats::StatsMetric {
    crate::capabilities::stats::StatsMetric {
        key: key.to_string(),
        label: label.to_string(),
        unit,
    }
}

pub(crate) fn count_metric(key: &str, label: &str) -> crate::capabilities::stats::StatsMetric {
    stat_metric(key, label, crate::capabilities::stats::StatsUnit::Count)
}

use crate::capabilities::calendar::CalendarEvent;
use crate::capabilities::dns_hole::DnsHoleSummary;
use crate::capabilities::downloads::{
    DownloadClientItem, DownloadClientJobsAndStatus, DownloadClientSelection,
};
use crate::capabilities::firewall::{
    FirewallCpuSummary, FirewallInterfacesSummary, FirewallMemorySummary, FirewallVersionSummary,
};
use crate::capabilities::health_monitoring::{ClusterHealthMonitoring, SystemHealthMonitoring};
use crate::capabilities::indexer_manager::Indexer;
use crate::capabilities::media_organizer::{MissingMediaPage, QueuedMediaPage};
use crate::capabilities::media_releases::MediaRelease;
use crate::capabilities::media_requests::{
    MediaInformation, MediaRequest, MediaRequestMediaType, RequestStats, RequestUser,
};
use crate::capabilities::media_server::{CurrentSessionsInput, StreamSession};
use crate::capabilities::media_transcoding::{TdarrQueue, TdarrStatistics, TdarrWorker};
use crate::capabilities::network_controller::NetworkControllerSummary;
use crate::capabilities::notifications::Notification;
use crate::capabilities::search::SearchResult;
use crate::capabilities::service_version::ServiceVersion;
use crate::capabilities::smart_home::{
    SmartHomeAction, SmartHomeActionReceipt, SmartHomeCatalog, SmartHomeEntityPatch,
    SmartHomeEntitySelection, SmartHomeForecastDay, SmartHomeForecastType, SmartHomeHistorySeries,
    SmartHomeSnapshot,
};
use crate::capabilities::stats::{StatsMetric, StatsValues};
use crate::capabilities::traefik::TraefikDashboardData;
use crate::capabilities::ups::UpsSummary;
use crate::capabilities::vpn::VpnSummary;
use crate::runtime::IntegrationRuntime;
use crate::types::{
    CalendarEvent as LegacyCalendarEvent, DnsHoleSummary as LegacyDnsHoleSummary,
    DownloadClientQueue, HealthMonitoringInfo, IndexerInfo, IntegrationContext, MediaOrganizerItem,
    MediaRequestItem, MediaRequestStats, MediaServerSession, MediaTranscodingSummary,
    NotificationItem, SmartHomeEntity as LegacySmartHomeEntity, StatsResponse,
    UpsSummary as LegacyUpsSummary,
};

pub mod downloads_network;
pub mod home_misc;
pub mod media;
pub mod mock;
pub mod stats;
pub mod systems;

/// `None` = this group does not handle the kind for this capability.
pub type Handled<T> = Option<HomarrResult<T>>;

#[async_trait]
pub trait IntegrationGroup: Send + Sync {
    /// The kinds owned by this group.
    fn kinds(&self) -> &'static [IntegrationKind];

    // ---- connection & metadata -------------------------------------------------------------

    /// Test connection for any kind of this group.
    async fn test_connection(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        None
    }

    /// `IServiceVersionIntegration.getServiceVersionAsync`.
    async fn service_version(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<ServiceVersion> {
        None
    }

    // ---- calendar --------------------------------------------------------------------------

    /// `ICalendarIntegration.getCalendarEventsAsync`.
    async fn calendar_events(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
        _include_unmonitored: bool,
    ) -> Handled<Vec<CalendarEvent>> {
        None
    }

    // ---- dns hole --------------------------------------------------------------------------

    async fn dns_hole_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<DnsHoleSummary> {
        None
    }

    async fn dns_hole_enable(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        None
    }

    /// `duration` in seconds; `None` disables until re-enabled.
    async fn dns_hole_disable(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _duration: Option<u64>,
    ) -> Handled<()> {
        None
    }

    // ---- downloads -------------------------------------------------------------------------

    async fn downloads_jobs_and_status(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> Handled<DownloadClientJobsAndStatus> {
        None
    }

    async fn downloads_pause_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        None
    }

    async fn downloads_resume_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        None
    }

    async fn downloads_pause_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
    ) -> Handled<()> {
        None
    }

    async fn downloads_resume_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
    ) -> Handled<()> {
        None
    }

    async fn downloads_delete_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
        _from_disk: bool,
    ) -> Handled<()> {
        None
    }

    // ---- firewall --------------------------------------------------------------------------

    async fn firewall_cpu(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallCpuSummary> {
        None
    }

    async fn firewall_memory(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallMemorySummary> {
        None
    }

    async fn firewall_interfaces(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<FirewallInterfacesSummary>> {
        None
    }

    async fn firewall_version(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<FirewallVersionSummary> {
        None
    }

    // ---- health monitoring -----------------------------------------------------------------

    /// `ISystemHealthMonitoringIntegration.getSystemInfoAsync`.
    async fn system_health(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<SystemHealthMonitoring> {
        None
    }

    /// `IClusterHealthMonitoringIntegration.getClusterInfoAsync`.
    async fn cluster_health(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<ClusterHealthMonitoring> {
        None
    }

    // ---- indexer manager -------------------------------------------------------------------

    async fn indexers(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<Indexer>> {
        None
    }

    async fn indexers_test_all(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<()> {
        None
    }

    // ---- media organizer -------------------------------------------------------------------

    async fn media_missing(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _page_size: Option<usize>,
    ) -> Handled<MissingMediaPage> {
        None
    }

    async fn media_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _page_size: Option<usize>,
    ) -> Handled<QueuedMediaPage> {
        None
    }

    // ---- media releases --------------------------------------------------------------------

    async fn media_releases(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaRelease>> {
        None
    }

    // ---- media requests --------------------------------------------------------------------

    async fn media_request_series_information(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _media_type: MediaRequestMediaType,
        _id: i64,
    ) -> Handled<MediaInformation> {
        None
    }

    async fn media_request_create(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _media_type: MediaRequestMediaType,
        _id: i64,
        _seasons: Option<&[i64]>,
    ) -> Handled<()> {
        None
    }

    async fn media_requests(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaRequest>> {
        None
    }

    async fn media_request_stats(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<RequestStats> {
        None
    }

    async fn media_request_users(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<RequestUser>> {
        None
    }

    async fn media_request_approve(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _request_id: i64,
    ) -> Handled<()> {
        None
    }

    async fn media_request_decline(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _request_id: i64,
    ) -> Handled<()> {
        None
    }

    // ---- media server ----------------------------------------------------------------------

    async fn media_server_sessions(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _options: CurrentSessionsInput,
    ) -> Handled<Vec<StreamSession>> {
        None
    }

    // ---- media transcoding -----------------------------------------------------------------

    async fn transcoding_statistics(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<TdarrStatistics> {
        None
    }

    async fn transcoding_workers(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<TdarrWorker>> {
        None
    }

    async fn transcoding_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _first_item_index: usize,
        _page_size: usize,
    ) -> Handled<TdarrQueue> {
        None
    }

    // ---- network controller ----------------------------------------------------------------

    async fn network_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<NetworkControllerSummary> {
        None
    }

    // ---- notifications ---------------------------------------------------------------------

    async fn notifications(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<Notification>> {
        None
    }

    // ---- search ----------------------------------------------------------------------------

    /// `ISearchableIntegration.searchAsync`.
    async fn search(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _query: &str,
    ) -> Handled<Vec<SearchResult>> {
        None
    }

    // ---- smart home ------------------------------------------------------------------------

    async fn smart_home_snapshot(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _selection: &SmartHomeEntitySelection,
    ) -> Handled<SmartHomeSnapshot> {
        None
    }

    async fn smart_home_catalog(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<SmartHomeCatalog> {
        None
    }

    async fn smart_home_history(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _entity_ids: &[String],
        _hours: u32,
        _samples: u32,
    ) -> Handled<Vec<SmartHomeHistorySeries>> {
        None
    }

    async fn smart_home_forecast(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _entity_id: &str,
        _forecast_type: SmartHomeForecastType,
    ) -> Handled<Vec<SmartHomeForecastDay>> {
        None
    }

    async fn smart_home_execute_action(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _entity_id: &str,
        _action: &SmartHomeAction,
    ) -> Handled<Option<SmartHomeActionReceipt>> {
        None
    }

    /// `subscribeEntitiesAsync`: pushes patches into `patches` until `cancel` flips to `true` or the stream ends.
    async fn smart_home_subscribe(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _selection: &SmartHomeEntitySelection,
        _patches: mpsc::UnboundedSender<SmartHomeEntityPatch>,
        _cancel: watch::Receiver<bool>,
    ) -> Handled<()> {
        None
    }

    // ---- stats -----------------------------------------------------------------------------

    /// Metric definitions (key, label, unit) of the stats provider for `kind`; empty when none.
    fn stats_metrics(&self, _kind: IntegrationKind) -> Vec<StatsMetric> {
        Vec::new()
    }

    /// `StatsProvider.fetchAsync`.
    async fn stats(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        None
    }

    // ---- ups -------------------------------------------------------------------------------

    async fn ups_summaries(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<UpsSummary>> {
        None
    }

    // ---- traefik ---------------------------------------------------------------------------

    async fn traefik_dashboard(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<TraefikDashboardData> {
        None
    }

    // ---- vpn -------------------------------------------------------------------------------

    async fn vpn_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<VpnSummary> {
        None
    }

    // ---- integration specific data ---------------------------------------------------------

    async fn integration_data(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _operation: &str,
        _input: &serde_json::Value,
    ) -> Handled<serde_json::Value> {
        None
    }

    // ---- legacy (pre-capability types, removed once consumers use the capability methods) ----
    //
    // These return the simplified types of `crate::types` that `IntegrationRegistry`'s existing public
    // methods expose. Groups implement them by delegating to their `legacy*` modules.

    async fn legacy_calendar_events(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _start: Option<&str>,
        _end: Option<&str>,
    ) -> Handled<Vec<LegacyCalendarEvent>> {
        None
    }

    async fn legacy_dns_hole_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<LegacyDnsHoleSummary> {
        None
    }

    async fn legacy_dns_hole_set_enabled(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _enabled: bool,
    ) -> Handled<()> {
        None
    }

    async fn legacy_downloads(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<DownloadClientQueue> {
        None
    }

    async fn legacy_media_sessions(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaServerSession>> {
        None
    }

    async fn legacy_media_requests(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<(Vec<MediaRequestItem>, MediaRequestStats)> {
        None
    }

    async fn legacy_media_request_action(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _request_id: &str,
        _approve: bool,
    ) -> Handled<()> {
        None
    }

    async fn legacy_health_monitoring(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<HealthMonitoringInfo> {
        None
    }

    async fn legacy_smart_home_entities(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<LegacySmartHomeEntity>> {
        None
    }

    async fn legacy_smart_home_action(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _entity_id: &str,
        _service: &str,
    ) -> Handled<()> {
        None
    }

    async fn legacy_notifications(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<NotificationItem>> {
        None
    }

    async fn legacy_indexers(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<IndexerInfo>> {
        None
    }

    async fn legacy_media_missing(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaOrganizerItem>> {
        None
    }

    async fn legacy_media_transcoding(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<MediaTranscodingSummary> {
        None
    }

    async fn legacy_ups_summary(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Vec<LegacyUpsSummary>> {
        None
    }

    async fn legacy_service_version(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<Option<String>> {
        None
    }

    async fn legacy_stats(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> Handled<StatsResponse> {
        None
    }
}

/// All groups, in dispatch order.
pub fn all_groups() -> &'static [&'static dyn IntegrationGroup] {
    static GROUPS: [&dyn IntegrationGroup; 6] = [
        &mock::MockGroup,
        &media::MediaGroup,
        &downloads_network::DownloadsNetworkGroup,
        &systems::SystemsGroup,
        &home_misc::HomeMiscGroup,
        &stats::StatsGroup,
    ];
    &GROUPS
}

/// The group owning `kind`.
pub fn group_for_kind(kind: IntegrationKind) -> Option<&'static dyn IntegrationGroup> {
    all_groups()
        .iter()
        .copied()
        .find(|g| g.kinds().contains(&kind))
}

#[cfg(test)]
mod tests {
    use homarr_definitions::integration::all_integration_defs;

    use super::*;

    #[test]
    fn every_kind_is_in_exactly_one_group() {
        for def in all_integration_defs() {
            let owners = all_groups()
                .iter()
                .filter(|g| g.kinds().contains(&def.kind))
                .count();
            assert_eq!(
                owners,
                1,
                "{} must belong to exactly one group, found {owners}",
                def.kind.as_str()
            );
        }
    }

    #[test]
    fn groups_do_not_list_unknown_kinds_twice() {
        for group in all_groups() {
            let kinds = group.kinds();
            for (i, kind) in kinds.iter().enumerate() {
                assert!(
                    !kinds[i + 1..].contains(kind),
                    "{} listed twice",
                    kind.as_str()
                );
            }
        }
    }

    #[test]
    fn stats_metrics_have_unique_keys_across_all_groups() {
        for def in all_integration_defs() {
            let group = group_for_kind(def.kind).expect("group for kind");
            let metrics = group.stats_metrics(def.kind);
            if matches!(def.kind, IntegrationKind::Ical | IntegrationKind::Bindery) {
                continue;
            }
            assert!(
                !metrics.is_empty(),
                "{} should expose stats metrics",
                def.kind.as_str()
            );
            let mut keys: Vec<_> = metrics.iter().map(|m| m.key.clone()).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(
                keys.len(),
                metrics.len(),
                "duplicate stats metric keys for {}",
                def.kind.as_str()
            );
        }
    }
}
