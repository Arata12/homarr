use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    cache: Arc<WidgetCache>,
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
            cache: Arc::default(),
        }
    }

    /// Resolves many widgets with bounded concurrency, preserving input order.
    pub async fn resolve_many(
        &self,
        reqs: Vec<(WidgetKind, Value, Vec<String>)>,
    ) -> Vec<HomarrResult<Value>> {
        use futures_util::stream::{self, StreamExt};
        stream::iter(reqs)
            .map(|(kind, options, ids)| async move {
                self.resolve_widget_data(kind, &options, &ids).await
            })
            .buffered(8)
            .collect()
            .await
    }

    /// Cached resolve (stale-while-revalidate) for kinds that hit external services.
    pub async fn resolve_widget_data(
        &self,
        kind: WidgetKind,
        options: &Value,
        integration_ids: &[String],
    ) -> HomarrResult<Value> {
        let Some(ttl) = cache_ttl(kind) else {
            return self.resolve_uncached(kind, options, integration_ids).await;
        };
        let key = cache_key(kind, options, integration_ids);
        match self.cache.lookup(&key, Instant::now()) {
            Lookup::Hit(value) => return Ok(value),
            Lookup::Stale(value) => {
                let this = self.clone();
                let options = options.clone();
                let ids = integration_ids.to_vec();
                tokio::spawn(async move {
                    let result = this.resolve_uncached(kind, &options, &ids).await;
                    this.cache.store(key, &result, ttl, Instant::now());
                });
                return Ok(value);
            }
            Lookup::Miss => {}
        }
        // shortcut: no single-flight, concurrent cold misses for one key all hit the integration; add a per-key lock if that shows up in load.
        let result = self.resolve_uncached(kind, options, integration_ids).await;
        self.cache.store(key, &result, ttl, Instant::now());
        result
    }

    async fn resolve_uncached(
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
            _ => {
                self.resolve_integration_widget(kind, options, integration_ids)
                    .await
            }
        }
    }

    async fn resolve_integration_widget(
        &self,
        kind: WidgetKind,
        options: &Value,
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
            W::MediaMissing => match self.integrations.get_media_missing(first).await {
                Ok(items) => Ok(json!({ "items": items })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::MediaReleases => match self.integrations.media_releases(first).await {
                Ok(releases) => Ok(json!({ "releases": releases })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
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
            W::Firewall => {
                let (cpu, memory, interfaces, version) = tokio::join!(
                    self.integrations.firewall_cpu(first),
                    self.integrations.firewall_memory(first),
                    self.integrations.firewall_interfaces(first),
                    self.integrations.firewall_version(first),
                );
                if let (Err(err), Err(_), Err(_), Err(_)) = (&cpu, &memory, &interfaces, &version) {
                    return Ok(json!({ "error": err.to_string() }));
                }
                Ok(json!({
                    "integrationName": first.name,
                    "cpu": cpu.ok(),
                    "memory": memory.ok(),
                    "interfaces": interfaces.ok(),
                    "version": version.ok(),
                }))
            }
            W::Vpn => match self.integrations.vpn_summary(first).await {
                Ok(vpn) => Ok(json!({ "integrationName": first.name, "vpn": vpn })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::Traefik => match self.integrations.traefik_dashboard(first).await {
                Ok(traefik) => Ok(json!({ "integrationName": first.name, "traefik": traefik })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
            W::NetworkControllerSummary | W::NetworkControllerStatus => {
                match self.integrations.network_summary(first).await {
                    Ok(network) => Ok(json!({ "integrationName": first.name, "network": network })),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::AnchorNote => {
                let note_id = options
                    .get("noteId")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let res = if let Some(id) = note_id {
                    self.integrations
                        .integration_data(first, "getNote", &json!({ "noteId": id }))
                        .await
                        .map(|note| json!({ "integrationName": first.name, "note": note }))
                } else {
                    self.integrations
                        .integration_data(first, "listNotes", &json!({ "limit": 5 }))
                        .await
                        .map(|notes| json!({ "integrationName": first.name, "notes": notes }))
                };
                match res {
                    Ok(val) => Ok(val),
                    Err(e) => Ok(json!({ "error": e.to_string() })),
                }
            }
            W::BeszelAlerts => {
                let empty_input = json!({});
                let (stats, alerts) = tokio::join!(
                    self.integrations.get_stats(first),
                    self.integrations
                        .integration_data(first, "getAlerts", &empty_input),
                );
                match (stats, alerts) {
                    (Ok(stats), alerts) => Ok(json!({
                        "integrationName": first.name,
                        "stats": stats,
                        "alerts": alerts.ok(),
                    })),
                    (Err(_), Ok(alerts)) => Ok(json!({
                        "integrationName": first.name,
                        "alerts": alerts,
                    })),
                    (Err(e), Err(_)) => Ok(json!({ "error": e.to_string() })),
                }
            }
            _ => match self.integrations.get_stats(first).await {
                Ok(stats) => Ok(json!({ "integrationName": first.name, "stats": stats })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            },
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

/// Upstream request-handler default (`DEFAULT_TTL_MS`).
const DEFAULT_TTL: Duration = Duration::from_secs(10);
/// Error payloads are kept briefly so a down integration is not hammered, but never served stale.
const ERROR_TTL: Duration = Duration::from_secs(5);
/// How long expired data may still be served while a background refresh runs.
const STALE_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_ENTRIES: usize = 512;

/// Per-kind TTL mirroring upstream `cacheTtlMs`; `None` = always resolve live.
fn cache_ttl(kind: WidgetKind) -> Option<Duration> {
    use WidgetKind as W;
    match kind {
        // Local/cheap kinds; App stays live so app and ping edits show immediately.
        // BeszelAlerts: upstream beszel stats use cacheTtlMs 0 (live polling).
        W::App
        | W::Clock
        | W::Bookmarks
        | W::Countdown
        | W::Timer
        | W::Notebook
        | W::Iframe
        | W::Video
        | W::Timetable
        | W::Assistant
        | W::BeszelAlerts => None,
        W::Weather | W::MediaReleases => Some(Duration::from_secs(5 * 60)),
        W::AirQuality => Some(Duration::from_secs(15 * 60)),
        W::Releases => Some(Duration::from_secs(6 * 60 * 60)),
        W::MediaRequestsRequestList | W::MediaRequestsRequestStats => Some(Duration::from_secs(60)),
        _ => Some(DEFAULT_TTL),
    }
}

/// Resolver output never depends on the caller (custom widgets run with a `None` user), so no user in the key.
fn cache_key(kind: WidgetKind, options: &Value, integration_ids: &[String]) -> String {
    format!(
        "{}|{}|{}",
        kind.as_str(),
        options,
        integration_ids.join(",")
    )
}

fn is_error_payload(value: &Value) -> bool {
    value.get("error").is_some() || value.get("missingIntegration").is_some()
}

struct CacheEntry {
    value: Value,
    fresh_until: Instant,
    stale_until: Instant,
    refreshing: bool,
}

enum Lookup {
    Hit(Value),
    /// Expired but within the stale window; the caller must refresh in the background.
    Stale(Value),
    Miss,
}

#[derive(Default)]
struct WidgetCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
}

impl WidgetCache {
    fn lookup(&self, key: &str, now: Instant) -> Lookup {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = entries.get_mut(key) else {
            return Lookup::Miss;
        };
        if now < entry.fresh_until {
            Lookup::Hit(entry.value.clone())
        } else if now < entry.stale_until {
            if entry.refreshing {
                Lookup::Hit(entry.value.clone())
            } else {
                entry.refreshing = true;
                Lookup::Stale(entry.value.clone())
            }
        } else {
            entries.remove(key);
            Lookup::Miss
        }
    }

    fn store(&self, key: String, result: &HomarrResult<Value>, ttl: Duration, now: Instant) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let value = match result {
            Ok(value) => value,
            Err(_) => {
                // Hard errors are not cached; let the next request retry the refresh.
                if let Some(entry) = entries.get_mut(&key) {
                    entry.refreshing = false;
                }
                return;
            }
        };
        let (fresh_until, stale_until) = if is_error_payload(value) {
            let until = now + ERROR_TTL.min(ttl);
            (until, until)
        } else {
            (now + ttl, now + ttl + STALE_WINDOW)
        };
        if entries.len() >= MAX_ENTRIES && !entries.contains_key(&key) {
            entries.retain(|_, e| e.stale_until > now);
            if entries.len() >= MAX_ENTRIES {
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, e)| e.stale_until)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(
            key,
            CacheEntry {
                value: value.clone(),
                fresh_until,
                stale_until,
                refreshing: false,
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_ttls_and_stale_while_revalidate() {
        assert!(cache_ttl(WidgetKind::App).is_none());
        assert!(cache_ttl(WidgetKind::Clock).is_none());
        assert_eq!(
            cache_ttl(WidgetKind::MediaReleases),
            Some(Duration::from_secs(300))
        );
        let opts = json!({ "a": 1 });
        let k1 = cache_key(WidgetKind::DnsHoleSummary, &opts, &["i1".to_string()]);
        assert_ne!(
            k1,
            cache_key(WidgetKind::DnsHoleSummary, &opts, &["i2".to_string()])
        );
        assert_ne!(
            k1,
            cache_key(
                WidgetKind::DnsHoleSummary,
                &json!({ "a": 2 }),
                &["i1".to_string()]
            )
        );

        let cache = WidgetCache::default();
        let t0 = Instant::now();
        let ttl = Duration::from_secs(60);
        cache.store(k1.clone(), &Ok(json!({ "items": [] })), ttl, t0);
        assert!(matches!(
            cache.lookup(&k1, t0 + Duration::from_secs(1)),
            Lookup::Hit(_)
        ));
        let later = t0 + ttl + Duration::from_secs(1);
        assert!(matches!(cache.lookup(&k1, later), Lookup::Stale(_)));
        // Only one caller triggers the refresh; others keep getting the stale value.
        assert!(matches!(cache.lookup(&k1, later), Lookup::Hit(_)));
        assert!(matches!(
            cache.lookup(&k1, t0 + ttl + STALE_WINDOW),
            Lookup::Miss
        ));

        // Error payloads expire after ERROR_TTL with no stale window.
        cache.store(k1.clone(), &Ok(json!({ "error": "down" })), ttl, t0);
        assert!(matches!(cache.lookup(&k1, t0), Lookup::Hit(_)));
        assert!(matches!(cache.lookup(&k1, t0 + ERROR_TTL), Lookup::Miss));
    }
}
