use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use cron::Schedule;
use homarr_common::{HomarrError, HomarrResult, SecretCrypto};
use homarr_core::AppConfig;
use homarr_db::{
    repos::{apps, integrations, settings},
    Database,
};
use homarr_definitions::IntegrationKind;
use homarr_integrations::{IntegrationContext, IntegrationRegistry};
use homarr_redis::RedisStore;
use homarr_services::ServiceContainer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobStatus {
    pub name: String,
    pub description: String,
    pub cron_expression: String,
    pub is_enabled: bool,
    pub is_running: bool,
    pub prevent_manual_execution: bool,
    pub prevent_custom_interval: bool,
    pub hidden: bool,
    pub last_run: Option<i64>,
    pub last_duration_ms: Option<u64>,
    pub last_error: Option<String>,
    pub next_run: Option<i64>,
}

struct JobSpec {
    name: &'static str,
    description: &'static str,
    default_cron: &'static str,
    run_on_start: bool,
    prevent_manual_execution: bool,
    prevent_custom_interval: bool,
    hidden: bool,
}

const BUILTIN_JOBS: &[JobSpec] = &[
    JobSpec {
        name: "ping",
        description: "Checks reachability of registered applications and URLs",
        default_cron: "0 * * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "iconsUpdater",
        description: "Syncs upstream Homarr, selfh.st, Papirus, and SimpleIcons catalogs",
        default_cron: "0 0 3 * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "analytics",
        description: "Computes internal instance usage snapshot",
        default_cron: "0 0 0 * * 0",
        run_on_start: false,
        prevent_manual_execution: true,
        prevent_custom_interval: true,
        hidden: true,
    },
    JobSpec {
        name: "mediaServer",
        description: "Refreshes active streams from configured media servers",
        default_cron: "*/30 * * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "downloads",
        description: "Refreshes torrent and Usenet download queues",
        default_cron: "*/30 * * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "dnsHole",
        description: "Refreshes Pi-hole / AdGuard Home DNS statistics",
        default_cron: "0 */2 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "healthMonitoring",
        description: "Polls Proxmox, Beszel, Glances, and Unraid system metrics",
        default_cron: "0 * * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "calendar",
        description: "Refreshes upcoming calendar events from Servarr & iCal",
        default_cron: "0 */15 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "indexerManager",
        description: "Checks Prowlarr and Jackett indexer health",
        default_cron: "0 */5 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "mediaRequests",
        description: "Refreshes Overseerr / Jellyseerr request lists",
        default_cron: "0 */2 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "smartHome",
        description: "Polls Home Assistant entity states",
        default_cron: "0 */2 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
    JobSpec {
        name: "releases",
        description: "Checks cached external software releases",
        default_cron: "0 */30 * * * *",
        run_on_start: false,
        prevent_manual_execution: false,
        prevent_custom_interval: false,
        hidden: false,
    },
];

/// Normalizes 5-field standard cron expressions to 6-field expressions required by `cron::Schedule`.
pub fn normalize_cron_expression(expr: &str) -> HomarrResult<String> {
    let trimmed = expr.split_whitespace().collect::<Vec<_>>();
    let normalized = match trimmed.len() {
        5 => format!("0 {}", trimmed.join(" ")),
        6 | 7 => trimmed.join(" "),
        _ => {
            return Err(HomarrError::BadRequest(format!(
                "Invalid cron expression '{expr}'; expected 5 or 6 fields"
            )));
        }
    };
    Schedule::from_str(&normalized)
        .map_err(|e| HomarrError::BadRequest(format!("Invalid cron expression '{expr}': {e}")))?;
    Ok(normalized)
}

fn next_run_timestamp(expr: &str, from: DateTime<Utc>) -> Option<i64> {
    Schedule::from_str(expr)
        .ok()
        .and_then(|sched| sched.after(&from).next())
        .map(|dt| dt.timestamp())
}

#[derive(Clone)]
pub struct TaskScheduler {
    config: AppConfig,
    db: Database,
    redis: RedisStore,
    crypto: SecretCrypto,
    integrations: IntegrationRegistry,
    services: ServiceContainer,
    jobs: Arc<RwLock<HashMap<String, CronJobStatus>>>,
    shutdown_tx: watch::Sender<bool>,
    handles: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
}

impl TaskScheduler {
    pub fn new(
        config: AppConfig,
        db: Database,
        redis: RedisStore,
        crypto: SecretCrypto,
        integrations: IntegrationRegistry,
        services: ServiceContainer,
    ) -> Self {
        let mut map = HashMap::new();
        let now = Utc::now();
        for spec in BUILTIN_JOBS {
            let expr = spec.default_cron.to_string();
            let next_run = next_run_timestamp(&expr, now);
            map.insert(
                spec.name.to_string(),
                CronJobStatus {
                    name: spec.name.to_string(),
                    description: spec.description.to_string(),
                    cron_expression: expr,
                    is_enabled: true,
                    is_running: false,
                    prevent_manual_execution: spec.prevent_manual_execution,
                    prevent_custom_interval: spec.prevent_custom_interval,
                    hidden: spec.hidden,
                    last_run: None,
                    last_duration_ms: None,
                    last_error: None,
                    next_run,
                },
            );
        }
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            config,
            db,
            redis,
            crypto,
            integrations,
            services,
            jobs: Arc::new(RwLock::new(map)),
            shutdown_tx,
            handles: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    pub async fn start(&self) -> HomarrResult<()> {
        // Apply DB cron overrides
        if let Ok(configs) = settings::list_cron_configs(&self.db).await {
            let mut guard = self.jobs.write().await;
            let now = Utc::now();
            for cfg in configs {
                if let Some(job) = guard.get_mut(&cfg.name) {
                    if let Ok(norm) = normalize_cron_expression(&cfg.cron_expression) {
                        job.cron_expression = norm;
                        job.next_run = next_run_timestamp(&job.cron_expression, now);
                    }
                    job.is_enabled = cfg.is_enabled;
                }
            }
        }

        // Run any `run_on_start` jobs once asynchronously
        for spec in BUILTIN_JOBS {
            if spec.run_on_start {
                let scheduler = self.clone();
                let name = spec.name.to_string();
                tokio::spawn(async move {
                    let _ = scheduler.execute_job_internal(&name).await;
                });
            }
        }

        let scheduler = self.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let now = Utc::now();
                        let now_ts = now.timestamp();
                        let due: Vec<String> = {
                            let guard = scheduler.jobs.read().await;
                            guard
                                .values()
                                .filter(|j| j.is_enabled && !j.is_running && j.next_run.map(|n| n <= now_ts).unwrap_or(false))
                                .map(|j| j.name.clone())
                                .collect()
                        };
                        for job_name in due {
                            let cloned = scheduler.clone();
                            tokio::spawn(async move {
                                let _ = cloned.execute_job_internal(&job_name).await;
                            });
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        self.handles.lock().await.push(handle);
        Ok(())
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        let mut handles = self.handles.lock().await;
        for h in handles.drain(..) {
            h.abort();
        }
    }

    pub async fn list_jobs(&self, include_hidden: bool) -> Vec<CronJobStatus> {
        let guard = self.jobs.read().await;
        let mut out: Vec<CronJobStatus> = guard
            .values()
            .filter(|j| include_hidden || !j.hidden)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub async fn trigger_job(&self, name: &str) -> HomarrResult<CronJobStatus> {
        {
            let guard = self.jobs.read().await;
            let job = guard
                .get(name)
                .ok_or_else(|| HomarrError::NotFound(format!("Cron job '{name}' not found")))?;
            if job.prevent_manual_execution {
                return Err(HomarrError::Forbidden(format!(
                    "Cron job '{name}' does not allow manual execution"
                )));
            }
        }
        self.execute_job_internal(name).await
    }

    pub async fn update_job(
        &self,
        name: &str,
        cron_expression: Option<&str>,
        is_enabled: Option<bool>,
    ) -> HomarrResult<CronJobStatus> {
        let updated = {
            let mut guard = self.jobs.write().await;
            let job = guard
                .get_mut(name)
                .ok_or_else(|| HomarrError::NotFound(format!("Cron job '{name}' not found")))?;
            if let Some(expr) = cron_expression {
                if job.prevent_custom_interval {
                    return Err(HomarrError::Forbidden(format!(
                        "Cron job '{name}' does not allow custom intervals"
                    )));
                }
                let normalized = normalize_cron_expression(expr)?;
                job.cron_expression = normalized;
                job.next_run = next_run_timestamp(&job.cron_expression, Utc::now());
            }
            if let Some(enabled) = is_enabled {
                job.is_enabled = enabled;
            }
            job.clone()
        };

        settings::upsert_cron_config(&self.db, name, &updated.cron_expression, updated.is_enabled)
            .await?;
        let _ = self
            .redis
            .publish_json(RedisStore::CRON_STATUS_CHANNEL, &updated)
            .await;
        Ok(updated)
    }

    async fn execute_job_internal(&self, name: &str) -> HomarrResult<CronJobStatus> {
        {
            let mut guard = self.jobs.write().await;
            let job = guard
                .get_mut(name)
                .ok_or_else(|| HomarrError::NotFound(format!("Cron job '{name}' not found")))?;
            if job.is_running {
                return Ok(job.clone());
            }
            job.is_running = true;
        }

        let start = Instant::now();
        let outcome = self.dispatch_job_work(name).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        let now = Utc::now();

        let status = {
            let mut guard = self.jobs.write().await;
            let job = guard
                .get_mut(name)
                .ok_or_else(|| HomarrError::NotFound(format!("Cron job '{name}' not found")))?;
            job.is_running = false;
            job.last_run = Some(now.timestamp());
            job.last_duration_ms = Some(duration_ms);
            job.last_error = outcome.err().map(|e| e.to_string());
            job.next_run = next_run_timestamp(&job.cron_expression, now);
            job.clone()
        };

        let _ = self
            .redis
            .set_json_ex(&format!("cron:status:{name}"), &status, 86_400)
            .await;
        let _ = self
            .redis
            .publish_json(RedisStore::CRON_STATUS_CHANNEL, &status)
            .await;

        Ok(status)
    }

    async fn dispatch_job_work(&self, name: &str) -> HomarrResult<()> {
        match name {
            "ping" => self.run_ping_job().await,
            "iconsUpdater" => {
                if !self.config.no_external_connection {
                    let _ = self.services.icons.sync_all(&self.db).await?;
                }
                Ok(())
            }
            "analytics" => {
                let snap = self
                    .services
                    .external
                    .build_analytics_snapshot(&self.db)
                    .await?;
                let _ = self
                    .redis
                    .set_json_ex("analytics:snapshot:latest", &snap, 604_800)
                    .await;
                Ok(())
            }
            "mediaServer" | "downloads" | "dnsHole" | "healthMonitoring" | "calendar"
            | "indexerManager" | "mediaRequests" | "smartHome" => {
                self.refresh_integrations_for_job(name).await
            }
            "releases" => {
                let _ = self.services.external.check_updates().await;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn run_ping_job(&self) -> HomarrResult<()> {
        if let Ok(board_cfg) = settings::get_setting(&self.db, "board").await {
            if board_cfg
                .get("forceDisableStatus")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return Ok(());
            }
        }

        let mut urls = self.redis.list_ping_urls().await.unwrap_or_default();
        if let Ok(all_apps) = apps::list_all(&self.db).await {
            for app in all_apps {
                if let Some(u) = app.ping_url.or(app.href) {
                    if !u.trim().is_empty() && !urls.contains(&u) {
                        urls.push(u);
                    }
                }
            }
        }

        for url in urls.into_iter().take(100) {
            let res = self.services.ping.ping_url(&url).await;
            let _ = self
                .redis
                .publish_json(RedisStore::PING_EVENTS_CHANNEL, &res)
                .await;
        }
        Ok(())
    }

    async fn refresh_integrations_for_job(&self, job: &str) -> HomarrResult<()> {
        let all = integrations::list_all(&self.db).await?;
        for rec in all {
            let Some(kind) = IntegrationKind::from_str_opt(&rec.kind) else {
                continue;
            };
            let matches = matches!(
                (job, kind),
                (
                    "mediaServer",
                    IntegrationKind::Jellyfin
                        | IntegrationKind::Emby
                        | IntegrationKind::Plex
                        | IntegrationKind::Mock
                ) | (
                    "downloads",
                    IntegrationKind::QBittorrent
                        | IntegrationKind::Transmission
                        | IntegrationKind::Deluge
                        | IntegrationKind::SabNzbd
                        | IntegrationKind::NzbGet
                        | IntegrationKind::Aria2
                        | IntegrationKind::Mock
                ) | (
                    "dnsHole",
                    IntegrationKind::PiHole
                        | IntegrationKind::AdGuardHome
                        | IntegrationKind::TechnitiumDns
                        | IntegrationKind::Mock
                ) | (
                    "healthMonitoring",
                    IntegrationKind::Proxmox
                        | IntegrationKind::Beszel
                        | IntegrationKind::Glances
                        | IntegrationKind::DashDot
                        | IntegrationKind::Unraid
                        | IntegrationKind::Mock
                ) | (
                    "calendar",
                    IntegrationKind::Sonarr
                        | IntegrationKind::Radarr
                        | IntegrationKind::Lidarr
                        | IntegrationKind::Readarr
                        | IntegrationKind::Ical
                        | IntegrationKind::Mock
                ) | (
                    "indexerManager",
                    IntegrationKind::Prowlarr | IntegrationKind::Jackett | IntegrationKind::Mock
                ) | (
                    "mediaRequests",
                    IntegrationKind::Overseerr
                        | IntegrationKind::Jellyseerr
                        | IntegrationKind::Seerr
                        | IntegrationKind::Mock
                ) | (
                    "smartHome",
                    IntegrationKind::HomeAssistant | IntegrationKind::Mock
                )
            );
            if !matches {
                continue;
            }
            let raw_secrets = integrations::get_secrets(&self.db, &rec.id)
                .await
                .unwrap_or_default();
            let mut secrets = HashMap::new();
            for s in raw_secrets {
                if let Ok(plain) = self.crypto.decrypt_secret(&s.value) {
                    secrets.insert(s.kind, plain);
                }
            }
            let ctx = IntegrationContext {
                id: rec.id.clone(),
                name: rec.name,
                url: rec.url,
                external_url: rec.external_url,
                kind,
                secrets,
            };
            let cached_val: Option<Value> = match job {
                "dnsHole" => self
                    .integrations
                    .get_dns_hole_summary(&ctx)
                    .await
                    .ok()
                    .and_then(|v| serde_json::to_value(v).ok()),
                "downloads" => self
                    .integrations
                    .get_downloads(&ctx)
                    .await
                    .ok()
                    .and_then(|v| serde_json::to_value(v).ok()),
                "mediaServer" => self
                    .integrations
                    .get_media_sessions(&ctx)
                    .await
                    .ok()
                    .and_then(|v| serde_json::to_value(v).ok()),
                "healthMonitoring" => self
                    .integrations
                    .get_health_monitoring(&ctx)
                    .await
                    .ok()
                    .and_then(|v| serde_json::to_value(v).ok()),
                _ => Some(json!({ "ok": true })),
            };
            if let Some(val) = cached_val {
                let _ = self
                    .redis
                    .set_json_ex(&format!("job:{job}:{}", rec.id), &val, 300)
                    .await;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_core::{AppConfig, CertificateManager, DatabaseDriver, HttpClientFactory};
    use homarr_db::seed::seed_defaults;

    #[test]
    fn normalizes_5_and_6_field_cron_expressions() {
        assert_eq!(
            normalize_cron_expression("* * * * *").unwrap(),
            "0 * * * * *"
        );
        assert_eq!(
            normalize_cron_expression("*/15 * * * * *").unwrap(),
            "*/15 * * * * *"
        );
        assert!(normalize_cron_expression("not-a-cron").is_err());
    }

    #[tokio::test]
    async fn lists_and_triggers_builtin_jobs() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        seed_defaults(&db).await.unwrap();

        let cfg = AppConfig::for_test();
        let certs = CertificateManager::new(cfg.local_certificate_path.clone());
        let http = HttpClientFactory::new(&certs).unwrap();
        let crypto = SecretCrypto::zero_for_test();
        let redis = RedisStore::in_memory();
        let integrations = IntegrationRegistry::new(http.clone());
        let services = ServiceContainer::new(cfg.clone(), crypto.clone(), http, redis.clone());

        let scheduler = TaskScheduler::new(cfg, db, redis, crypto, integrations, services);
        let visible = scheduler.list_jobs(false).await;
        let all = scheduler.list_jobs(true).await;
        assert!(visible.iter().any(|j| j.name == "ping"));
        assert!(!visible.iter().any(|j| j.name == "analytics"));
        assert!(all.iter().any(|j| j.name == "analytics"));

        // Manual execution on analytics is forbidden
        assert!(scheduler.trigger_job("analytics").await.is_err());

        // Trigger ping manually succeeds cleanly
        let ping_status = scheduler.trigger_job("ping").await.unwrap();
        assert!(ping_status.last_run.is_some());
        assert!(!ping_status.is_running);

        // Update cron interval
        let updated = scheduler
            .update_job("ping", Some("*/2 * * * *"), Some(false))
            .await
            .unwrap();
        assert_eq!(updated.cron_expression, "0 */2 * * * *");
        assert!(!updated.is_enabled);
    }
}
