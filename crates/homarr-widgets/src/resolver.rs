use std::collections::HashMap;

use chrono::Utc;
use homarr_common::{HomarrResult, SecretCrypto};
use homarr_db::{
    repos::{apps, custom_widgets as cw_repo, integrations as int_repo},
    Database,
};
use homarr_definitions::{IntegrationKind, WidgetKind};
use homarr_integrations::{IntegrationContext, IntegrationRegistry};
use homarr_services::{render_template, ServiceContainer};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct WidgetResolver {
    db: Database,
    crypto: SecretCrypto,
    integrations: IntegrationRegistry,
    services: ServiceContainer,
}

impl WidgetResolver {
    pub fn new(
        db: Database,
        crypto: SecretCrypto,
        integrations: IntegrationRegistry,
        services: ServiceContainer,
    ) -> Self {
        Self {
            db,
            crypto,
            integrations,
            services,
        }
    }

    pub async fn resolve_widget_data(
        &self,
        kind: WidgetKind,
        options: &Value,
        integration_ids: &[String],
    ) -> HomarrResult<Value> {
        use WidgetKind as W;
        let opts = options.as_object();

        match kind {
            W::App => {
                let app_id = opts
                    .and_then(|o| o.get("appId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if app_id.is_empty() {
                    return Ok(json!({ "app": null }));
                }
                let app = apps::get_by_id(&self.db, app_id).await?;
                let ping = if let Some(ref a) = app {
                    let target = a.ping_url.as_deref().or(a.href.as_deref());
                    if let Some(u) = target {
                        self.services.ping.get_cached_ping(u).await.ok().flatten()
                    } else {
                        None
                    }
                } else {
                    None
                };
                Ok(json!({ "app": app, "ping": ping }))
            }
            W::Clock => {
                let now = Utc::now();
                Ok(json!({
                    "iso": now.to_rfc3339(),
                    "time24": now.format("%H:%M").to_string(),
                    "time12": now.format("%I:%M %p").to_string(),
                    "date": now.format("%a, %b %d").to_string(),
                }))
            }
            W::Weather => {
                let loc = opts.and_then(|o| o.get("location"));
                let lat = loc
                    .and_then(|l| l.get("latitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(47.3769);
                let lon = loc
                    .and_then(|l| l.get("longitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(8.5417);
                let name = loc
                    .and_then(|l| l.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Zurich");
                match self.services.external.get_weather(lat, lon).await {
                    Ok(w) => Ok(json!({ "locationName": name, "weather": w })),
                    Err(err) => Ok(json!({ "locationName": name, "error": err.to_string() })),
                }
            }
            W::AirQuality => {
                let loc = opts.and_then(|o| o.get("location"));
                let lat = loc
                    .and_then(|l| l.get("latitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(47.3769);
                let lon = loc
                    .and_then(|l| l.get("longitude"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(8.5417);
                let name = loc
                    .and_then(|l| l.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Zurich");
                match self.services.external.get_air_quality(lat, lon).await {
                    Ok(aq) => Ok(json!({ "locationName": name, "airQuality": aq })),
                    Err(err) => Ok(json!({ "locationName": name, "error": err.to_string() })),
                }
            }
            W::RssFeed => {
                let urls: Vec<String> = opts
                    .and_then(|o| o.get("feedUrls"))
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let limit = opts
                    .and_then(|o| o.get("maximumAmountOfPosts"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(8) as usize;
                match self.services.external.fetch_feeds(&urls, limit).await {
                    Ok(entries) => Ok(json!({ "entries": entries })),
                    Err(err) => Ok(json!({ "entries": [], "error": err.to_string() })),
                }
            }
            W::StockPrice => {
                let symbol = opts
                    .and_then(|o| o.get("stock"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("NVDA");
                match self.services.external.get_stock_price(symbol).await {
                    Ok(stock) => Ok(json!({ "stock": stock })),
                    Err(err) => Ok(json!({ "symbol": symbol, "error": err.to_string() })),
                }
            }
            W::MinecraftServerStatus => {
                let domain = opts
                    .and_then(|o| o.get("domain"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("localhost:25565");
                let (host, port) = parse_host_port(domain, 25565);
                let status = self
                    .services
                    .external
                    .ping_minecraft_server(&host, port)
                    .await;
                Ok(json!({ "status": status }))
            }
            W::Releases => {
                let provider = opts
                    .and_then(|o| o.get("provider"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("github");
                let identifier = opts
                    .and_then(|o| o.get("identifier"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("homarr-labs/homarr");
                match self
                    .services
                    .external
                    .get_latest_release(provider, identifier)
                    .await
                {
                    Ok(rel) => Ok(json!({ "release": rel })),
                    Err(err) => Ok(json!({ "error": err.to_string() })),
                }
            }
            W::DockerContainers => {
                let containers = self
                    .services
                    .docker
                    .list_containers()
                    .await
                    .unwrap_or_default();
                Ok(json!({ "containers": containers }))
            }
            W::CustomApi => {
                let cw_id = opts
                    .and_then(|o| o.get("customWidgetId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !cw_id.is_empty() {
                    if let Some(def) = cw_repo::get_v2_by_id(&self.db, cw_id).await? {
                        let req_id = def
                            .requests
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|r| r.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let opt_map: HashMap<String, Value> = opts
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default();
                        let exec = if !req_id.is_empty() {
                            self.services
                                .custom_widgets
                                .execute_request(
                                    &self.db,
                                    &self.crypto,
                                    &self.integrations,
                                    // API callers authorize options.integrationId before resolving.
                                    None,
                                    &def.id,
                                    req_id,
                                    &opt_map,
                                )
                                .await
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }))
                        } else {
                            json!({})
                        };
                        let html = render_template(&def.template, &exec, &opt_map);
                        return Ok(json!({
                            "name": def.name,
                            "renderedHtml": html,
                            "data": exec,
                        }));
                    }
                }
                Ok(json!({ "options": options }))
            }
            W::Bookmarks
            | W::Countdown
            | W::Timer
            | W::Notebook
            | W::Iframe
            | W::Video
            | W::Timetable
            | W::Assistant => Ok(json!({ "options": options })),
            _ => self.resolve_integration_widget(kind, integration_ids).await,
        }
    }

    async fn resolve_integration_widget(
        &self,
        kind: WidgetKind,
        integration_ids: &[String],
    ) -> HomarrResult<Value> {
        let contexts = self.load_integration_contexts(integration_ids).await?;
        let Some(first) = contexts.first() else {
            return Ok(json!({ "missingIntegration": true }));
        };

        use WidgetKind as W;
        match kind {
            W::Calendar => {
                let mut events = Vec::new();
                for ctx in &contexts {
                    if let Ok(mut evs) =
                        self.integrations.get_calendar_events(ctx, None, None).await
                    {
                        events.append(&mut evs);
                    }
                }
                Ok(json!({ "events": events }))
            }
            W::Downloads => match self.integrations.get_downloads(first).await {
                Ok(q) => Ok(json!({ "queue": q })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::MediaServer => {
                let mut sessions = Vec::new();
                for ctx in &contexts {
                    if let Ok(mut s) = self.integrations.get_media_sessions(ctx).await {
                        sessions.append(&mut s);
                    }
                }
                Ok(json!({ "sessions": sessions }))
            }
            W::MediaRequestsRequestList | W::MediaRequestsRequestStats => {
                match self.integrations.get_media_requests(first).await {
                    Ok((items, stats)) => Ok(json!({ "items": items, "stats": stats })),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::MediaTranscoding => match self.integrations.get_media_transcoding(first).await {
                Ok(summary) => Ok(json!({ "summary": summary })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::MediaMissing | W::MediaReleases => {
                match self.integrations.get_media_missing(first).await {
                    Ok(items) => Ok(json!({ "items": items })),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::IndexerManager => match self.integrations.get_indexers(first).await {
                Ok(indexers) => Ok(json!({ "indexers": indexers })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::DnsHoleSummary | W::DnsHoleControls => {
                let mut summaries = Vec::new();
                for ctx in &contexts {
                    if let Ok(s) = self.integrations.get_dns_hole_summary(ctx).await {
                        summaries.push(json!({
                            "integrationId": ctx.id,
                            "name": ctx.name,
                            "summary": s,
                        }));
                    }
                }
                Ok(json!({ "items": summaries }))
            }
            W::HealthMonitoring | W::SystemResources | W::SystemDisks => {
                match self.integrations.get_health_monitoring(first).await {
                    Ok(info) => Ok(json!({ "health": info })),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::SmartHomeEntityState | W::SmartHomeExecuteAutomation | W::SmartHomeControls => {
                match self.integrations.get_smart_home_entities(first).await {
                    Ok(entities) => Ok(json!({ "entities": entities })),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::Notifications => match self.integrations.get_notifications(first).await {
                Ok(items) => Ok(json!({ "notifications": items })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::Ups => match self.integrations.get_ups_summary(first).await {
                Ok(items) => Ok(json!({ "ups": items })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::Stats => match self.integrations.get_stats(first).await {
                Ok(stats) => Ok(json!({ "stats": stats })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            _ => {
                let payload = self
                    .integrations
                    .get_generic_widget_data(first, kind.as_str())
                    .await?;
                Ok(json!({
                    "integrationName": first.name,
                    "data": payload
                }))
            }
        }
    }

    pub async fn load_integration_contexts(
        &self,
        integration_ids: &[String],
    ) -> HomarrResult<Vec<IntegrationContext>> {
        let mut contexts = Vec::new();
        for id in integration_ids {
            if let Some(rec) = int_repo::get_by_id(&self.db, id).await? {
                let Some(kind) = IntegrationKind::from_str_opt(&rec.kind) else {
                    continue;
                };
                let raw_secrets = int_repo::get_secrets(&self.db, &rec.id).await?;
                let mut secrets = HashMap::new();
                for s in raw_secrets {
                    if let Ok(plain) = self.crypto.decrypt_secret(&s.value) {
                        secrets.insert(s.kind, plain);
                    }
                }
                contexts.push(IntegrationContext {
                    id: rec.id,
                    name: rec.name,
                    url: rec.url,
                    external_url: rec.external_url,
                    kind,
                    secrets,
                });
            }
        }
        Ok(contexts)
    }
}

fn parse_host_port(raw: &str, default_port: u16) -> (String, u16) {
    let trimmed = raw.trim();
    if let Some((h, p)) = trimmed.rsplit_once(':') {
        if let Ok(port) = p.parse::<u16>() {
            return (h.to_string(), port);
        }
    }
    (trimmed.to_string(), default_port)
}
