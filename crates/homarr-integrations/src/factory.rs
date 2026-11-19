use std::collections::HashMap;

use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use reqwest::Method;
use serde_json::{json, Value};

use crate::clients::{
    build_request_url, dns_hole, downloads, health, media, mock, network_misc, servarr, stats,
};
use crate::http_auth::{resolve_declared_http_auth, ResolvedHttpAuth};
use crate::types::{
    CalendarEvent, DnsHoleSummary, DownloadClientQueue, HealthMonitoringInfo, HttpProxyResponse,
    IndexerInfo, IntegrationContext, MediaOrganizerItem, MediaRequestItem, MediaRequestStats,
    MediaServerSession, MediaTranscodingSummary, NotificationItem, SmartHomeEntity, StatsResponse,
    TestConnectionResponse, UpsSummary,
};

#[derive(Clone)]
pub struct IntegrationRegistry {
    http: HttpClientFactory,
}

impl IntegrationRegistry {
    pub fn new(http: HttpClientFactory) -> Self {
        Self { http }
    }

    pub fn http(&self) -> &HttpClientFactory {
        &self.http
    }

    pub fn resolve_http_auth(&self, ctx: &IntegrationContext) -> HomarrResult<ResolvedHttpAuth> {
        resolve_declared_http_auth(ctx)
    }

    pub async fn test_connection(&self, ctx: &IntegrationContext) -> TestConnectionResponse {
        use IntegrationKind as K;
        let res = match ctx.kind {
            K::Mock => Ok(()),
            K::Sonarr
            | K::Sportarr
            | K::Radarr
            | K::Lidarr
            | K::Readarr
            | K::Bindery
            | K::Prowlarr
            | K::Bazarr
            | K::Jackett => servarr::test_connection(&self.http, ctx).await,
            K::SabNzbd
            | K::NzbGet
            | K::Deluge
            | K::Transmission
            | K::QBittorrent
            | K::Aria2
            | K::Slskd => downloads::test_connection(&self.http, ctx).await,
            K::Jellyfin
            | K::Emby
            | K::Plex
            | K::Jellyseerr
            | K::Seerr
            | K::Overseerr
            | K::Tdarr
            | K::Audiobookshelf
            | K::Navidrome
            | K::Immich => media::test_connection(&self.http, ctx).await,
            K::PiHole | K::AdGuardHome | K::TechnitiumDns => {
                dns_hole::test_connection(&self.http, ctx).await
            }
            K::Proxmox
            | K::DashDot
            | K::Glances
            | K::Coolify
            | K::Unraid
            | K::UptimeKuma
            | K::Wud
            | K::Llamacpp
            | K::PeaNut
            | K::Beszel
            | K::Patchmon
            | K::Synology
            | K::Openmediavault
            | K::Truenas => health::test_connection(&self.http, ctx).await,
            K::HomeAssistant
            | K::Opnsense
            | K::Traefik
            | K::Gluetun
            | K::Ntfy
            | K::Gotify
            | K::PaperlessNgx
            | K::Anchor
            | K::SpeedtestTracker
            | K::Tracearr
            | K::Thingport
            | K::Umami
            | K::ArchiveTeamWarrior
            | K::UnifiController
            | K::Wazuh
            | K::Nextcloud
            | K::Ical => network_misc::test_connection(&self.http, ctx).await,
            _ => stats::test_connection(&self.http, ctx).await,
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

        let mut req = self.http.standard().request(req_method, url);
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

        let resp = req
            .send()
            .await
            .map_err(|err| HomarrError::Upstream(format!("Proxy request failed: {err}")))?;
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

    pub async fn get_calendar_events(
        &self,
        ctx: &IntegrationContext,
        start: Option<&str>,
        end: Option<&str>,
    ) -> HomarrResult<Vec<CalendarEvent>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::calendar_events()),
            IntegrationKind::Ical => network_misc::get_ical_events(&self.http, ctx).await,
            IntegrationKind::Sonarr
            | IntegrationKind::Radarr
            | IntegrationKind::Lidarr
            | IntegrationKind::Readarr
            | IntegrationKind::Sportarr => {
                servarr::get_calendar_events(&self.http, ctx, start, end).await
            }
            _ => Ok(Vec::new()),
        }
    }

    pub async fn get_dns_hole_summary(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DnsHoleSummary> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::dns_hole_summary()),
            _ => dns_hole::get_summary(&self.http, ctx).await,
        }
    }

    pub async fn set_dns_hole_enabled(
        &self,
        ctx: &IntegrationContext,
        enabled: bool,
    ) -> HomarrResult<()> {
        if ctx.kind == IntegrationKind::Mock {
            return Ok(());
        }
        dns_hole::set_enabled(&self.http, ctx, enabled).await
    }

    pub async fn get_downloads(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<DownloadClientQueue> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::downloads()),
            _ => downloads::get_queue(&self.http, ctx).await,
        }
    }

    pub async fn get_media_sessions(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<MediaServerSession>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::media_sessions()),
            _ => media::get_current_sessions(&self.http, ctx).await,
        }
    }

    pub async fn get_media_requests(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<(Vec<MediaRequestItem>, MediaRequestStats)> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::media_requests()),
            _ => media::get_requests(&self.http, ctx).await,
        }
    }

    pub async fn media_request_action(
        &self,
        ctx: &IntegrationContext,
        request_id: &str,
        approve: bool,
    ) -> HomarrResult<()> {
        if ctx.kind == IntegrationKind::Mock {
            return Ok(());
        }
        media::approve_or_decline_request(&self.http, ctx, request_id, approve).await
    }

    pub async fn get_health_monitoring(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<HealthMonitoringInfo> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::health_monitoring()),
            _ => health::get_health_info(&self.http, ctx).await,
        }
    }

    pub async fn get_smart_home_entities(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<SmartHomeEntity>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::smart_home_entities()),
            _ => network_misc::get_smart_home_entities(&self.http, ctx).await,
        }
    }

    pub async fn execute_smart_home_action(
        &self,
        ctx: &IntegrationContext,
        entity_id: &str,
        service: &str,
    ) -> HomarrResult<()> {
        if ctx.kind == IntegrationKind::Mock {
            return Ok(());
        }
        network_misc::execute_smart_home_action(&self.http, ctx, entity_id, service).await
    }

    pub async fn get_notifications(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<NotificationItem>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::notifications()),
            _ => network_misc::get_notifications(&self.http, ctx).await,
        }
    }

    pub async fn get_indexers(&self, ctx: &IntegrationContext) -> HomarrResult<Vec<IndexerInfo>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::indexers()),
            _ => servarr::get_indexers(&self.http, ctx).await,
        }
    }

    pub async fn get_media_missing(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Vec<MediaOrganizerItem>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::media_missing()),
            _ => servarr::get_missing(&self.http, ctx).await,
        }
    }

    pub async fn get_media_transcoding(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<MediaTranscodingSummary> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::media_transcoding()),
            _ => media::get_tdarr_summary(&self.http, ctx).await,
        }
    }

    pub async fn get_ups_summary(&self, ctx: &IntegrationContext) -> HomarrResult<Vec<UpsSummary>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(mock::ups_summary()),
            _ => health::get_ups_summary(&self.http, ctx).await,
        }
    }

    pub async fn get_service_version(
        &self,
        ctx: &IntegrationContext,
    ) -> HomarrResult<Option<String>> {
        match ctx.kind {
            IntegrationKind::Mock => Ok(Some("3.0.0-mock".into())),
            IntegrationKind::Sonarr
            | IntegrationKind::Radarr
            | IntegrationKind::Lidarr
            | IntegrationKind::Readarr
            | IntegrationKind::Bindery
            | IntegrationKind::Prowlarr => servarr::get_service_version(&self.http, ctx).await,
            _ => Ok(None),
        }
    }

    pub async fn get_stats(&self, ctx: &IntegrationContext) -> HomarrResult<StatsResponse> {
        if ctx.kind == IntegrationKind::Mock {
            return Ok(mock::stats(&ctx.name));
        }
        stats::fetch_stats(&self.http, ctx).await
    }

    pub async fn get_generic_widget_data(
        &self,
        ctx: &IntegrationContext,
        widget_kind: &str,
    ) -> HomarrResult<Value> {
        if ctx.kind == IntegrationKind::Mock {
            return Ok(mock::generic_widget_data(widget_kind));
        }
        let path = match widget_kind {
            "firewall" => "/api/core/system/status",
            "traefik" => "/api/overview",
            "vpn" => "/v1/publicip/ip",
            "coolify" => "/api/v1/applications",
            "paperlessNgx" => "/api/statistics/",
            "speedtestTracker" => "/api/v1/results/latest",
            "wud" => "/api/containers",
            "llamacpp" => "/health",
            "bazarr" => "/api/system/status",
            "networkControllerSummary" | "networkControllerStatus" => "/status",
            _ => "/",
        };
        match crate::clients::get_json(&self.http, ctx, path).await {
            Ok(v) => Ok(v),
            Err(err) => Ok(json!({ "error": err.to_string() })),
        }
    }
}
