use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use reqwest::Method;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::capabilities::calendar::CalendarEvent as CapCalendarEvent;
use crate::capabilities::dns_hole::DnsHoleSummary as CapDnsHoleSummary;
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
use crate::capabilities::ups::UpsSummary as CapUpsSummary;
use crate::capabilities::vpn::VpnSummary;
use crate::clients::build_request_url;
use crate::error::unsupported;
use crate::groups::{group_for_kind, IntegrationGroup};
use crate::http_auth::{resolve_declared_http_auth, ResolvedHttpAuth};
use crate::runtime::{ImageProxyProvider, IntegrationRuntime};
use crate::types::{
    CalendarEvent, DnsHoleSummary, DownloadClientQueue, HealthMonitoringInfo, HttpProxyResponse,
    IndexerInfo, IntegrationContext, MediaOrganizerItem, MediaRequestItem, MediaRequestStats,
    MediaServerSession, MediaTranscodingSummary, NotificationItem, SmartHomeEntity, StatsResponse,
    TestConnectionResponse, UpsSummary,
};

#[derive(Clone)]
pub struct IntegrationRegistry {
    rt: IntegrationRuntime,
}

fn group_of(
    ctx: &IntegrationContext,
    capability: &str,
) -> HomarrResult<&'static dyn IntegrationGroup> {
    group_for_kind(ctx.kind).ok_or_else(|| unsupported(ctx, capability))
}

/// A group answering `None` means it does not implement the capability for this kind.
fn resolved<T>(
    ctx: &IntegrationContext,
    capability: &str,
    out: Option<HomarrResult<T>>,
) -> HomarrResult<T> {
    out.unwrap_or_else(|| Err(unsupported(ctx, capability)))
}

/// Generates a thin public wrapper dispatching a capability method to the group owning `ctx.kind`.
macro_rules! capability {
    ($name:ident, $label:literal, $method:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty) => {
        pub async fn $name(&self, ctx: &IntegrationContext $(, $arg: $ty)*) -> HomarrResult<$ret> {
            let group = group_of(ctx, $label)?;
            resolved(ctx, $label, group.$method(&self.rt, ctx $(, $arg)*).await)
        }
    };
}

impl IntegrationRegistry {
    pub fn new(http: HttpClientFactory) -> Self {
        Self {
            rt: IntegrationRuntime::new(http),
        }
    }

    pub fn with_image_proxy(
        http: HttpClientFactory,
        image_proxy: Arc<dyn ImageProxyProvider>,
    ) -> Self {
        Self {
            rt: IntegrationRuntime::with_image_proxy(http, image_proxy),
        }
    }

    pub fn http(&self) -> &HttpClientFactory {
        &self.rt.http
    }

    pub fn runtime(&self) -> &IntegrationRuntime {
        &self.rt
    }

    pub fn resolve_http_auth(&self, ctx: &IntegrationContext) -> HomarrResult<ResolvedHttpAuth> {
        resolve_declared_http_auth(ctx)
    }

    pub async fn test_connection(&self, ctx: &IntegrationContext) -> TestConnectionResponse {
        let res = match group_of(ctx, "test connection") {
            Ok(group) => resolved(
                ctx,
                "test connection",
                group.test_connection(&self.rt, ctx).await,
            ),
            Err(err) => Err(err),
        };

        match res {
            Ok(()) => TestConnectionResponse {
                success: true,
                error: None,
            },
            Err(err) => TestConnectionResponse {
                success: false,
                error: Some(err.to_string()),
            },
        }
    }

    pub async fn execute_http_request(
        &self,
        ctx: &IntegrationContext,
        method: &str,
        path: &str,
        extra_headers: Option<&HashMap<String, String>>,
        extra_query: Option<&HashMap<String, String>>,
        body: Option<Value>,
    ) -> HomarrResult<HttpProxyResponse> {
        if !homarr_definitions::is_http_integration_kind(ctx.kind) {
            return Err(HomarrError::BadRequest(format!(
                "Integration kind '{}' does not support generic HTTP requests",
                ctx.kind.as_str()
            )));
        }

        let auth = self.resolve_http_auth(ctx)?;
        let mut query = auth.query;
        if let Some(eq) = extra_query {
            for (k, v) in eq {
                query.insert(k.clone(), v.clone());
            }
        }
        let url = build_request_url(ctx.base_url(), path, &query)?;
        let req_method = Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes())
            .map_err(|_| HomarrError::BadRequest(format!("Invalid HTTP method: {method}")))?;

        let mut req = self.rt.http.standard().request(req_method, url);
        if let Some(eh) = extra_headers {
            for (k, v) in eh {
                req = req.header(k, v);
            }
        }
        for (k, v) in &auth.headers {
            req = req.header(k, v);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await.map_err(|err| {
            HomarrError::Upstream(format!("Proxy request failed: {}", err.without_url()))
        })?;
        let status = resp.status().as_u16();
        let mut resp_headers = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(val_str) = v.to_str() {
                resp_headers.insert(k.as_str().to_string(), val_str.to_string());
            }
        }
        let text = resp.text().await.unwrap_or_default();
        let body_val = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));

        Ok(HttpProxyResponse {
            status,
            headers: resp_headers,
            body: body_val,
        })
    }

    // ---- existing public API (simplified `crate::types` shapes; consumers are rewired in Phase 2) ----

    pub async fn get_calendar_events(
        &self,
        ctx: &IntegrationContext,
        start: Option<&str>,
        end: Option<&str>,
    ) -> HomarrResult<Vec<CalendarEvent>> {
        let group = group_of(ctx, "calendar")?;
        resolved(
            ctx,
            "calendar",
            group
                .legacy_calendar_events(&self.rt, ctx, start, end)
                .await,
        )
    }

    pub async fn get_dns_hole_summary(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary> {
        let group = group_of(ctx, "DNS hole summary")?;
        resolved(
            ctx,
            "DNS hole summary",
            group.legacy_dns_hole_summary(&self.rt, ctx).await,
        )
    }

    pub async fn set_dns_hole_enabled(
        &self,
        ctx: &IntegrationContext,
        enabled: bool,
    ) -> HomarrResult<()> {
        let group = group_of(ctx, "DNS hole control")?;
        resolved(
            ctx,
            "DNS hole control",
            group
                .legacy_dns_hole_set_enabled(&self.rt, ctx, enabled)
                .await,
        )
    }

    pub async fn get_downloads(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DownloadClientQueue> {
        let group = group_of(ctx, "downloads")?;
        resolved(
            ctx,
            "downloads",
            group.legacy_downloads(&self.rt, ctx).await,
        )
    }

    pub async fn get_media_sessions(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<MediaServerSession>> {
        let group = group_of(ctx, "media server sessions")?;
        resolved(
            ctx,
            "media server sessions",
            group.legacy_media_sessions(&self.rt, ctx).await,
        )
    }

    pub async fn get_media_requests(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<(Vec<MediaRequestItem>, MediaRequestStats)> {
        let group = group_of(ctx, "media requests")?;
        resolved(
            ctx,
            "media requests",
            group.legacy_media_requests(&self.rt, ctx).await,
        )
    }

    pub async fn media_request_action(
        &self,
        ctx: &IntegrationContext,
        request_id: &str,
        approve: bool,
    ) -> HomarrResult<()> {
        let group = group_of(ctx, "media requests")?;
        resolved(
            ctx,
            "media requests",
            group
                .legacy_media_request_action(&self.rt, ctx, request_id, approve)
                .await,
        )
    }

    pub async fn get_health_monitoring(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<HealthMonitoringInfo> {
        let group = group_of(ctx, "health monitoring")?;
        resolved(
            ctx,
            "health monitoring",
            group.legacy_health_monitoring(&self.rt, ctx).await,
        )
    }

    pub async fn get_smart_home_entities(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<SmartHomeEntity>> {
        let group = group_of(ctx, "smart home")?;
        resolved(
            ctx,
            "smart home",
            group.legacy_smart_home_entities(&self.rt, ctx).await,
        )
    }

    pub async fn execute_smart_home_action(
        &self,
        ctx: &IntegrationContext,
        entity_id: &str,
        service: &str,
    ) -> HomarrResult<()> {
        let group = group_of(ctx, "smart home")?;
        resolved(
            ctx,
            "smart home",
            group
                .legacy_smart_home_action(&self.rt, ctx, entity_id, service)
                .await,
        )
    }

    pub async fn get_notifications(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<NotificationItem>> {
        let group = group_of(ctx, "notifications")?;
        resolved(
            ctx,
            "notifications",
            group.legacy_notifications(&self.rt, ctx).await,
        )
    }

    pub async fn get_indexers(&self, ctx: &IntegrationContext) -> HomarrResult<Vec<IndexerInfo>> {
        let group = group_of(ctx, "indexer manager")?;
        resolved(
            ctx,
            "indexer manager",
            group.legacy_indexers(&self.rt, ctx).await,
        )
    }

    pub async fn get_media_missing(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<MediaOrganizerItem>> {
        let group = group_of(ctx, "media organizer")?;
        resolved(
            ctx,
            "media organizer",
            group.legacy_media_missing(&self.rt, ctx).await,
        )
    }

    pub async fn get_media_transcoding(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<MediaTranscodingSummary> {
        let group = group_of(ctx, "media transcoding")?;
        resolved(
            ctx,
            "media transcoding",
            group.legacy_media_transcoding(&self.rt, ctx).await,
        )
    }

    pub async fn get_ups_summary(&self, ctx: &IntegrationContext) -> HomarrResult<Vec<UpsSummary>> {
        let group = group_of(ctx, "UPS summary")?;
        resolved(
            ctx,
            "UPS summary",
            group.legacy_ups_summary(&self.rt, ctx).await,
        )
    }

    pub async fn get_service_version(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Option<String>> {
        let group = group_of(ctx, "service version")?;
        resolved(
            ctx,
            "service version",
            group.legacy_service_version(&self.rt, ctx).await,
        )
    }

    pub async fn get_stats(&self, ctx: &IntegrationContext) -> HomarrResult<StatsResponse> {
        let group = group_of(ctx, "stats")?;
        if let Some(result) = group.stats(&self.rt, ctx).await {
            let values = result?;
            let metrics = group.stats_metrics(ctx.kind);
            if !metrics.is_empty() {
                return Ok(crate::groups::stats::format::to_legacy_stats_response(
                    ctx, &metrics, &values,
                ));
            }
        }
        match group.legacy_stats(&self.rt, ctx).await {
            Some(result) => result,
            // The legacy generic probe works for any kind, not only the ones the stats group owns.
            None => crate::groups::stats::legacy_fetch_any(&self.rt, ctx).await,
        }
    }

    pub fn format_stats(
        &self,
        ctx: &IntegrationContext,
        metrics: &[StatsMetric],
        values: &StatsValues,
    ) -> StatsResponse {
        crate::groups::stats::format::to_legacy_stats_response(ctx, metrics, values)
    }

    // ---- capability API (TS-shaped types from `crate::capabilities`) -------------------------------

    capability!(service_version, "service version", service_version() -> ServiceVersion);

    capability!(
        calendar_events,
        "calendar",
        calendar_events(
            start: DateTime<Utc>,
            end: DateTime<Utc>,
            include_unmonitored: bool,
        ) -> Vec<CapCalendarEvent>
    );

    capability!(dns_hole_summary, "DNS hole summary", dns_hole_summary() -> CapDnsHoleSummary);
    capability!(dns_hole_enable, "DNS hole control", dns_hole_enable() -> ());
    capability!(
        dns_hole_disable,
        "DNS hole control",
        dns_hole_disable(duration: Option<u64>) -> ()
    );

    capability!(
        downloads_jobs_and_status,
        "downloads",
        downloads_jobs_and_status(
            limit: usize,
            selection: Option<&DownloadClientSelection>,
        ) -> DownloadClientJobsAndStatus
    );
    capability!(downloads_pause_queue, "downloads", downloads_pause_queue() -> ());
    capability!(downloads_resume_queue, "downloads", downloads_resume_queue() -> ());
    capability!(
        downloads_pause_item,
        "downloads",
        downloads_pause_item(item: &DownloadClientItem) -> ()
    );
    capability!(
        downloads_resume_item,
        "downloads",
        downloads_resume_item(item: &DownloadClientItem) -> ()
    );
    capability!(
        downloads_delete_item,
        "downloads",
        downloads_delete_item(item: &DownloadClientItem, from_disk: bool) -> ()
    );

    capability!(firewall_cpu, "firewall", firewall_cpu() -> FirewallCpuSummary);
    capability!(firewall_memory, "firewall", firewall_memory() -> FirewallMemorySummary);
    capability!(firewall_interfaces, "firewall", firewall_interfaces() -> Vec<FirewallInterfacesSummary>);
    capability!(firewall_version, "firewall", firewall_version() -> FirewallVersionSummary);

    capability!(system_health, "system health monitoring", system_health() -> SystemHealthMonitoring);
    capability!(cluster_health, "cluster health monitoring", cluster_health() -> ClusterHealthMonitoring);

    capability!(indexers, "indexer manager", indexers() -> Vec<Indexer>);
    capability!(indexers_test_all, "indexer manager", indexers_test_all() -> ());

    capability!(
        media_missing,
        "media organizer",
        media_missing(page_size: Option<usize>) -> MissingMediaPage
    );
    capability!(
        media_queue,
        "media organizer",
        media_queue(page_size: Option<usize>) -> QueuedMediaPage
    );

    capability!(media_releases, "media releases", media_releases() -> Vec<MediaRelease>);

    capability!(
        media_request_series_information,
        "media requests",
        media_request_series_information(
            media_type: MediaRequestMediaType,
            id: i64,
        ) -> MediaInformation
    );
    capability!(
        media_request_create,
        "media requests",
        media_request_create(
            media_type: MediaRequestMediaType,
            id: i64,
            seasons: Option<&[i64]>,
        ) -> ()
    );
    capability!(media_requests, "media requests", media_requests() -> Vec<MediaRequest>);
    capability!(media_request_stats, "media requests", media_request_stats() -> RequestStats);
    capability!(media_request_users, "media requests", media_request_users() -> Vec<RequestUser>);
    capability!(
        media_request_approve,
        "media requests",
        media_request_approve(request_id: i64) -> ()
    );
    capability!(
        media_request_decline,
        "media requests",
        media_request_decline(request_id: i64) -> ()
    );

    capability!(
        media_server_sessions,
        "media server sessions",
        media_server_sessions(options: CurrentSessionsInput) -> Vec<StreamSession>
    );

    capability!(transcoding_statistics, "media transcoding", transcoding_statistics() -> TdarrStatistics);
    capability!(transcoding_workers, "media transcoding", transcoding_workers() -> Vec<TdarrWorker>);
    capability!(
        transcoding_queue,
        "media transcoding",
        transcoding_queue(first_item_index: usize, page_size: usize) -> TdarrQueue
    );

    capability!(network_summary, "network controller summary", network_summary() -> NetworkControllerSummary);

    capability!(notifications, "notifications", notifications() -> Vec<Notification>);

    capability!(search, "search", search(query: &str) -> Vec<SearchResult>);

    capability!(
        smart_home_snapshot,
        "smart home",
        smart_home_snapshot(selection: &SmartHomeEntitySelection) -> SmartHomeSnapshot
    );
    capability!(smart_home_catalog, "smart home", smart_home_catalog() -> SmartHomeCatalog);
    capability!(
        smart_home_history,
        "smart home",
        smart_home_history(
            entity_ids: &[String],
            hours: u32,
            samples: u32,
        ) -> Vec<SmartHomeHistorySeries>
    );
    capability!(
        smart_home_forecast,
        "smart home",
        smart_home_forecast(
            entity_id: &str,
            forecast_type: SmartHomeForecastType,
        ) -> Vec<SmartHomeForecastDay>
    );
    capability!(
        smart_home_execute_action,
        "smart home",
        smart_home_execute_action(
            entity_id: &str,
            action: &SmartHomeAction,
        ) -> Option<SmartHomeActionReceipt>
    );
    capability!(
        smart_home_subscribe,
        "smart home",
        smart_home_subscribe(
            selection: &SmartHomeEntitySelection,
            patches: mpsc::UnboundedSender<SmartHomeEntityPatch>,
            cancel: watch::Receiver<bool>,
        ) -> ()
    );

    capability!(
        integration_data,
        "integration data",
        integration_data(operation: &str, input: &Value) -> Value
    );

    capability!(stats_values, "stats", stats() -> StatsValues);

    /// Metric definitions of the stats provider for `kind` (empty when the kind has none).
    pub fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        group_for_kind(kind)
            .map(|group| group.stats_metrics(kind))
            .unwrap_or_default()
    }

    capability!(ups_summaries, "UPS summary", ups_summaries() -> Vec<CapUpsSummary>);

    capability!(vpn_summary, "VPN summary", vpn_summary() -> VpnSummary);
    capability!(traefik_dashboard, "Traefik dashboard", traefik_dashboard() -> TraefikDashboardData);
}
