//! Systems group: health monitoring and friends (Proxmox, Glances, DashDot, Unraid, ...).
//!
//! One file per integration. Kinds without a capability slot in `IntegrationGroup` (Coolify, Uptime Kuma, WUD,
//! llama.cpp, PatchMon, Beszel) expose their typed data through plain functions of their module; the legacy
//! `stats` adapter below maps them onto the simplified `StatsResponse`.

use async_trait::async_trait;
use homarr_definitions::IntegrationKind;

use super::{Handled, IntegrationGroup};
use crate::capabilities::health_monitoring::{ClusterHealthMonitoring, SystemHealthMonitoring};
use crate::capabilities::service_version::ServiceVersion;
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::capabilities::ups::UpsSummary;
use crate::error::unsupported;
use crate::runtime::IntegrationRuntime;
use crate::types::{
    ClusterNodeInfo, HealthMonitoringDisk, HealthMonitoringInfo, IntegrationContext, StatMetric,
    StatsResponse, UpsSummary as LegacyUpsSummary,
};
use homarr_common::{HomarrError, HomarrResult};
use serde_json::{json, Value};

mod beszel;
mod coolify;
mod dashdot;
mod glances;
mod http;
mod llamacpp;
mod openmediavault;
mod patchmon;
mod peanut;
mod proxmox;
mod synology;
mod truenas;
mod unraid;
mod uptime_kuma;
mod wud;

pub struct SystemsGroup;

const KINDS: &[IntegrationKind] = &[
    IntegrationKind::Proxmox,
    IntegrationKind::DashDot,
    IntegrationKind::Glances,
    IntegrationKind::Coolify,
    IntegrationKind::Unraid,
    IntegrationKind::UptimeKuma,
    IntegrationKind::Wud,
    IntegrationKind::Llamacpp,
    IntegrationKind::PeaNut,
    IntegrationKind::Beszel,
    IntegrationKind::Patchmon,
    IntegrationKind::Synology,
    IntegrationKind::Openmediavault,
    IntegrationKind::Truenas,
];

#[async_trait]
impl IntegrationGroup for SystemsGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(match ctx.kind {
            IntegrationKind::Proxmox => proxmox::test_connection(rt, ctx).await,
            IntegrationKind::DashDot => dashdot::test_connection(rt, ctx).await,
            IntegrationKind::Glances => glances::test_connection(rt, ctx).await,
            IntegrationKind::Coolify => coolify::test_connection(rt, ctx).await,
            IntegrationKind::Unraid => unraid::test_connection(rt, ctx).await,
            IntegrationKind::UptimeKuma => uptime_kuma::test_connection(rt, ctx).await,
            IntegrationKind::Wud => wud::test_connection(rt, ctx).await,
            IntegrationKind::Llamacpp => llamacpp::test_connection(rt, ctx).await,
            IntegrationKind::PeaNut => peanut::test_connection(rt, ctx).await,
            IntegrationKind::Beszel => beszel::test_connection(rt, ctx).await,
            IntegrationKind::Patchmon => patchmon::test_connection(rt, ctx).await,
            IntegrationKind::Synology => synology::test_connection(rt, ctx).await,
            IntegrationKind::Openmediavault => openmediavault::test_connection(rt, ctx).await,
            IntegrationKind::Truenas => truenas::test_connection(rt, ctx).await,
            _ => return None,
        })
    }

    async fn service_version(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<ServiceVersion> {
        (ctx.kind == IntegrationKind::Coolify).then_some(())?;
        Some(
            coolify::version(rt, ctx)
                .await
                .map(|version| ServiceVersion { version }),
        )
    }

    async fn system_health(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<SystemHealthMonitoring> {
        Some(match ctx.kind {
            IntegrationKind::DashDot => dashdot::system_health(rt, ctx).await,
            IntegrationKind::Glances => glances::system_health(rt, ctx).await,
            IntegrationKind::Unraid => unraid::system_health(rt, ctx).await,
            IntegrationKind::Synology => synology::system_health(rt, ctx).await,
            IntegrationKind::Openmediavault => openmediavault::system_health(rt, ctx).await,
            IntegrationKind::Truenas => truenas::system_health(rt, ctx).await,
            _ => return None,
        })
    }

    async fn cluster_health(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<ClusterHealthMonitoring> {
        (ctx.kind == IntegrationKind::Proxmox).then_some(())?;
        Some(proxmox::cluster_health(rt, ctx).await)
    }

    async fn ups_summaries(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<UpsSummary>> {
        (ctx.kind == IntegrationKind::PeaNut).then_some(())?;
        Some(peanut::ups_summaries(rt, ctx).await)
    }

    async fn integration_data(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        operation: &str,
        input: &Value,
    ) -> Handled<Value> {
        fn to_json(value: impl serde::Serialize) -> HomarrResult<Value> {
            serde_json::to_value(value)
                .map_err(|err| HomarrError::Internal(format!("Serialization error: {err}")))
        }
        fn req_str<'a>(
            ctx: &IntegrationContext,
            input: &'a Value,
            key: &str,
        ) -> HomarrResult<&'a str> {
            input
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    HomarrError::BadRequest(format!("{key} is required for {}", ctx.name))
                })
        }
        match ctx.kind {
            IntegrationKind::Coolify
            | IntegrationKind::UptimeKuma
            | IntegrationKind::Wud
            | IntegrationKind::Patchmon
            | IntegrationKind::Llamacpp
            | IntegrationKind::Beszel => {}
            _ => return None,
        }
        Some(
            async {
                match ctx.kind {
                    IntegrationKind::Coolify => match operation {
                        "getInstanceInfo" => to_json(coolify::instance_info(rt, ctx).await?),
                        "getApplications" => to_json(coolify::applications(rt, ctx).await?),
                        "getProjects" => to_json(coolify::projects(rt, ctx).await?),
                        "getServers" => to_json(coolify::servers(rt, ctx).await?),
                        "getServices" => to_json(coolify::services(rt, ctx).await?),
                        _ => Err(unsupported(ctx, operation)),
                    },
                    IntegrationKind::UptimeKuma => match operation {
                        "getStatusPage" => {
                            let slug = input.get("slug").and_then(Value::as_str);
                            to_json(uptime_kuma::status_page(rt, ctx, slug).await?)
                        }
                        _ => Err(unsupported(ctx, operation)),
                    },
                    IntegrationKind::Wud => match operation {
                        "getStats" => to_json(wud::stats(rt, ctx).await?),
                        _ => Err(unsupported(ctx, operation)),
                    },
                    IntegrationKind::Patchmon => match operation {
                        "getStats" => to_json(patchmon::stats(rt, ctx).await?),
                        _ => Err(unsupported(ctx, operation)),
                    },
                    IntegrationKind::Llamacpp => match operation {
                        "getStats" => to_json(llamacpp::stats(rt, ctx).await?),
                        _ => Err(unsupported(ctx, operation)),
                    },
                    IntegrationKind::Beszel => match operation {
                        "getSystems" => to_json(beszel::systems(rt, ctx).await?),
                        "getSystemDetails" => {
                            let system_id = req_str(ctx, input, "systemId")?;
                            to_json(beszel::system_details(rt, ctx, system_id).await?)
                        }
                        "getSystemStats" => {
                            let system_id = req_str(ctx, input, "systemId")?;
                            let stats_type = req_str(ctx, input, "statsType")?;
                            let per_page = input
                                .get("perPage")
                                .and_then(Value::as_u64)
                                .unwrap_or(60)
                                .clamp(1, 500) as u32;
                            to_json(
                                beszel::system_stats(rt, ctx, system_id, stats_type, per_page)
                                    .await?,
                            )
                        }
                        "getAlerts" => {
                            let system_id = input.get("systemId").and_then(Value::as_str);
                            to_json(beszel::alerts(rt, ctx, system_id).await?)
                        }
                        "getAlertHistory" => {
                            let system_id = input.get("systemId").and_then(Value::as_str);
                            let per_page = input
                                .get("perPage")
                                .and_then(Value::as_u64)
                                .unwrap_or(60)
                                .clamp(1, 500) as u32;
                            to_json(beszel::alert_history(rt, ctx, system_id, per_page).await?)
                        }
                        "updateAlert" => {
                            let alert_id = req_str(ctx, input, "alertId")?;
                            let payload = input
                                .get("payload")
                                .cloned()
                                .unwrap_or_else(|| input.clone());
                            to_json(beszel::update_alert(rt, ctx, alert_id, payload).await?)
                        }
                        "deleteAlert" => {
                            let alert_id = req_str(ctx, input, "alertId")?;
                            beszel::delete_alert(rt, ctx, alert_id).await?;
                            Ok(json!({ "ok": true }))
                        }
                        "pauseSystem" => {
                            let system_id = req_str(ctx, input, "systemId")?;
                            beszel::pause_system(rt, ctx, system_id).await?;
                            Ok(json!({ "ok": true }))
                        }
                        "resumeSystem" => {
                            let system_id = req_str(ctx, input, "systemId")?;
                            beszel::resume_system(rt, ctx, system_id).await?;
                            Ok(json!({ "ok": true }))
                        }
                        "deleteSystem" => {
                            let system_id = req_str(ctx, input, "systemId")?;
                            beszel::delete_system(rt, ctx, system_id).await?;
                            Ok(json!({ "ok": true }))
                        }
                        _ => Err(unsupported(ctx, operation)),
                    },
                    _ => unreachable!(),
                }
            }
            .await,
        )
    }

    fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        use super::{count_metric, stat_metric};
        use IntegrationKind as K;
        match kind {
            K::DashDot | K::Glances | K::Openmediavault | K::Synology | K::Truenas | K::Unraid => {
                vec![
                    stat_metric("cpu", "CPU", StatsUnit::Percent),
                    stat_metric("memoryUsed", "Memory used", StatsUnit::Bytes),
                    stat_metric("memoryAvailable", "Memory available", StatsUnit::Bytes),
                    stat_metric("uptime", "Uptime", StatsUnit::Seconds),
                ]
            }
            K::Proxmox => vec![
                count_metric("nodes", "Nodes"),
                count_metric("vms", "Virtual machines"),
                count_metric("lxcs", "Containers"),
            ],
            K::Wud => vec![
                count_metric("totalContainers", "Containers"),
                count_metric("updatesAvailable", "Updates available"),
            ],
            K::Patchmon => vec![
                count_metric("totalHosts", "Hosts"),
                count_metric("hostsNeedingUpdates", "Hosts needing updates"),
                count_metric("securityUpdates", "Security updates"),
                count_metric("totalOutdatedPackages", "Outdated packages"),
            ],
            K::UptimeKuma => vec![
                count_metric("totalMonitors", "Monitors"),
                count_metric("upCount", "Up"),
                count_metric("downCount", "Down"),
                count_metric("pendingCount", "Pending"),
                count_metric("maintenanceCount", "Maintenance"),
                count_metric("staleCount", "No recent data"),
                stat_metric("averageUptimePercent", "Average uptime", StatsUnit::Percent),
            ],
            K::Coolify => vec![
                count_metric("applications", "Applications"),
                count_metric("services", "Services"),
                count_metric("servers", "Servers"),
            ],
            K::Beszel => vec![count_metric("systems", "Systems")],
            K::PeaNut => vec![
                count_metric("devices", "UPS devices"),
                count_metric("onBattery", "On battery"),
                count_metric("lowBattery", "Low battery"),
            ],
            K::Llamacpp => vec![
                stat_metric("health", "Health", StatsUnit::Text),
                count_metric("tokensGenerated", "Tokens generated"),
                count_metric("requestsProcessing", "Requests processing"),
            ],
            _ => Vec::new(),
        }
    }

    async fn stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        use IntegrationKind as K;
        if let Some(health) = self.system_health(rt, ctx).await {
            return Some(health.map(|data| {
                StatsValues::from([
                    ("cpu".into(), StatsValue::Number(data.cpu_utilization)),
                    (
                        "memoryUsed".into(),
                        StatsValue::Number(data.mem_used_in_bytes),
                    ),
                    (
                        "memoryAvailable".into(),
                        StatsValue::Number(data.mem_available_in_bytes),
                    ),
                    ("uptime".into(), StatsValue::Number(data.uptime)),
                ])
            }));
        }
        Some(match ctx.kind {
            K::Proxmox => proxmox::cluster_health(rt, ctx).await.map(|data| {
                StatsValues::from([
                    ("nodes".into(), StatsValue::Number(data.nodes.len() as f64)),
                    ("vms".into(), StatsValue::Number(data.vms.len() as f64)),
                    ("lxcs".into(), StatsValue::Number(data.lxcs.len() as f64)),
                ])
            }),
            K::Wud => wud::stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    (
                        "totalContainers".into(),
                        StatsValue::Number(s.total_containers as f64),
                    ),
                    (
                        "updatesAvailable".into(),
                        StatsValue::Number(s.updates_available as f64),
                    ),
                ])
            }),
            K::Patchmon => patchmon::stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    ("totalHosts".into(), StatsValue::Number(s.total_hosts)),
                    (
                        "hostsNeedingUpdates".into(),
                        StatsValue::Number(s.hosts_needing_updates),
                    ),
                    (
                        "securityUpdates".into(),
                        StatsValue::Number(s.security_updates),
                    ),
                    (
                        "totalOutdatedPackages".into(),
                        StatsValue::Number(s.total_outdated_packages),
                    ),
                ])
            }),
            K::UptimeKuma => uptime_kuma::status_page(rt, ctx, None).await.map(|page| {
                let summary = uptime_kuma::summarize(&uptime_kuma::unique_monitors(&page.groups));
                let count = |s| summary.counts.get(&s).copied().unwrap_or(0) as f64;
                StatsValues::from([
                    (
                        "totalMonitors".into(),
                        StatsValue::Number(summary.total as f64),
                    ),
                    (
                        "upCount".into(),
                        StatsValue::Number(count(uptime_kuma::MonitorStatus::Up)),
                    ),
                    (
                        "downCount".into(),
                        StatsValue::Number(count(uptime_kuma::MonitorStatus::Down)),
                    ),
                    (
                        "pendingCount".into(),
                        StatsValue::Number(count(uptime_kuma::MonitorStatus::Pending)),
                    ),
                    (
                        "maintenanceCount".into(),
                        StatsValue::Number(count(uptime_kuma::MonitorStatus::Maintenance)),
                    ),
                    (
                        "staleCount".into(),
                        StatsValue::Number(count(uptime_kuma::MonitorStatus::Stale)),
                    ),
                    (
                        "averageUptimePercent".into(),
                        summary
                            .average_uptime24h
                            .map_or(StatsValue::Null, StatsValue::Number),
                    ),
                ])
            }),
            K::Coolify => coolify::instance_info(rt, ctx).await.map(|data| {
                StatsValues::from([
                    (
                        "applications".into(),
                        StatsValue::Number(data.applications.len() as f64),
                    ),
                    (
                        "services".into(),
                        StatsValue::Number(data.services.len() as f64),
                    ),
                    (
                        "servers".into(),
                        StatsValue::Number(data.servers.len() as f64),
                    ),
                ])
            }),
            K::Beszel => beszel::systems(rt, ctx).await.map(|systems| {
                StatsValues::from([("systems".into(), StatsValue::Number(systems.len() as f64))])
            }),
            K::PeaNut => peanut::ups_summaries(rt, ctx).await.map(|data| {
                use crate::capabilities::ups::UpsStatus;
                let on_battery = data
                    .iter()
                    .filter(|item| item.status == UpsStatus::OnBattery)
                    .count();
                let low_battery = data
                    .iter()
                    .filter(|item| item.status == UpsStatus::LowBattery)
                    .count();
                StatsValues::from([
                    ("devices".into(), StatsValue::Number(data.len() as f64)),
                    ("onBattery".into(), StatsValue::Number(on_battery as f64)),
                    ("lowBattery".into(), StatsValue::Number(low_battery as f64)),
                ])
            }),
            K::Llamacpp => llamacpp::stats(rt, ctx).await.map(|data| {
                StatsValues::from([
                    ("health".into(), StatsValue::Text(data.health)),
                    (
                        "tokensGenerated".into(),
                        data.metrics
                            .tokens_generated
                            .map_or(StatsValue::Null, StatsValue::Number),
                    ),
                    (
                        "requestsProcessing".into(),
                        data.metrics
                            .requests_processing
                            .map_or(StatsValue::Null, StatsValue::Number),
                    ),
                ])
            }),
            _ => return None,
        })
    }

    async fn legacy_health_monitoring(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<HealthMonitoringInfo> {
        if ctx.kind == IntegrationKind::Proxmox {
            return Some(
                proxmox::cluster_health(rt, ctx)
                    .await
                    .map(|cluster| legacy_cluster_info(&cluster)),
            );
        }
        let health = self.system_health(rt, ctx).await?;
        Some(health.map(|health| legacy_system_info(&health)))
    }

    async fn legacy_ups_summary(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<LegacyUpsSummary>> {
        let summaries = self.ups_summaries(rt, ctx).await?;
        Some(summaries.map(|list| list.iter().map(legacy_ups).collect()))
    }

    async fn legacy_service_version(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Option<String>> {
        let version = self.service_version(rt, ctx).await?;
        Some(version.map(|v| Some(v.version)))
    }

    async fn legacy_stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsResponse> {
        let metrics = match ctx.kind {
            IntegrationKind::Wud => wud::stats(rt, ctx).await.map(|s| {
                vec![
                    metric("Containers", s.total_containers, None),
                    metric("Updates available", s.updates_available, None),
                ]
            }),
            IntegrationKind::Patchmon => patchmon::stats(rt, ctx).await.map(|s| {
                vec![
                    metric("Hosts", s.total_hosts, None),
                    metric("Hosts needing updates", s.hosts_needing_updates, None),
                    metric("Security updates", s.security_updates, None),
                    metric("Outdated packages", s.total_outdated_packages, None),
                ]
            }),
            IntegrationKind::Llamacpp => llamacpp::stats(rt, ctx).await.map(llamacpp_metrics),
            IntegrationKind::UptimeKuma => {
                uptime_kuma::status_page(rt, ctx, None).await.map(|page| {
                    let summary =
                        uptime_kuma::summarize(&uptime_kuma::unique_monitors(&page.groups));
                    let count = |s| summary.counts.get(&s).copied().unwrap_or(0);
                    let mut metrics = vec![
                        metric("Monitors", summary.total, None),
                        metric("Up", count(uptime_kuma::MonitorStatus::Up), None),
                        metric("Down", count(uptime_kuma::MonitorStatus::Down), None),
                    ];
                    if let Some(uptime) = summary.average_uptime24h {
                        metrics.push(metric("Uptime (24h)", format!("{uptime:.2}"), Some("%")));
                    }
                    metrics
                })
            }
            IntegrationKind::Coolify => coolify::instance_info(rt, ctx).await.map(|info| {
                vec![
                    metric("Applications", info.applications.len(), None),
                    metric("Services", info.services.len(), None),
                    metric("Servers", info.servers.len(), None),
                ]
            }),
            IntegrationKind::Beszel => beszel::systems(rt, ctx).await.map(|systems| {
                let mut metrics = vec![metric("Systems", systems.len(), None)];
                metrics.extend(
                    beszel::count_by_status(&systems)
                        .into_iter()
                        .map(|(status, count)| metric(&capitalize(&status), count, None)),
                );
                metrics
            }),
            _ => return None,
        };
        Some(metrics.map(|metrics| StatsResponse {
            integration_name: ctx.name.clone(),
            kind: ctx.kind.as_str().to_string(),
            metrics,
        }))
    }
}

fn metric(label: &str, value: impl ToString, unit: Option<&str>) -> StatMetric {
    StatMetric {
        label: label.to_string(),
        value: value.to_string(),
        unit: unit.map(str::to_string),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn llamacpp_metrics(stats: llamacpp::LlamacppStats) -> Vec<StatMetric> {
    let mut metrics = vec![metric("Health", &stats.health, None)];
    if let Some(model) = &stats.model {
        metrics.push(metric("Model", &model.name, None));
    }
    if let Some(usage) = &stats.context_usage {
        metrics.push(metric("Context usage", usage.percent, Some("%")));
    }
    let tps = |label: &str, value: Option<f64>, metrics: &mut Vec<StatMetric>| {
        if let Some(value) = value {
            metrics.push(metric(label, format!("{value:.1}"), Some("tok/s")));
        }
    };
    tps(
        "Generation speed",
        stats.metrics.generation_speed_tps,
        &mut metrics,
    );
    tps("Prompt speed", stats.metrics.prompt_speed_tps, &mut metrics);
    metrics
}

fn legacy_ups(ups: &UpsSummary) -> LegacyUpsSummary {
    use crate::capabilities::ups::UpsStatus;
    LegacyUpsSummary {
        name: ups.name.clone(),
        status: match ups.status {
            UpsStatus::Online => "OL",
            UpsStatus::Charging => "OL CHRG",
            UpsStatus::OnBattery => "OB",
            UpsStatus::LowBattery => "OB LB",
            UpsStatus::Unknown => "UNKNOWN",
        }
        .to_string(),
        battery_charge_percent: ups.battery_charge.unwrap_or(0.0),
        load_percent: ups.load.unwrap_or(0.0),
        runtime_seconds: ups.battery_runtime.unwrap_or(0.0).max(0.0) as u64,
    }
}

fn parse_bytes(value: &str) -> u64 {
    value
        .trim()
        .parse::<f64>()
        .map(|v| v.max(0.0) as u64)
        .unwrap_or(0)
}

fn legacy_system_info(health: &SystemHealthMonitoring) -> HealthMonitoringInfo {
    HealthMonitoringInfo {
        system_name: health.cpu_model_name.clone(),
        version: health.version.clone(),
        cpu_percent: health.cpu_utilization,
        cpu_temp_c: health.cpu_temp,
        memory_used_bytes: health.mem_used_in_bytes.max(0.0) as u64,
        memory_total_bytes: (health.mem_used_in_bytes + health.mem_available_in_bytes).max(0.0)
            as u64,
        uptime_seconds: health.uptime.max(0.0) as u64,
        load_average: health
            .load_average
            .as_ref()
            .map(|l| [l.one_min, l.five_min, l.fifteen_min]),
        disks: health
            .file_system
            .iter()
            .map(|fs| {
                let used = parse_bytes(&fs.used);
                HealthMonitoringDisk {
                    name: fs.device_name.clone(),
                    mount_point: fs.device_name.clone(),
                    used_bytes: used,
                    total_bytes: fs
                        .size
                        .map(|s| s.max(0.0) as u64)
                        .unwrap_or_else(|| used + parse_bytes(&fs.available)),
                }
            })
            .collect(),
        nodes: Vec::new(),
    }
}

fn legacy_cluster_info(cluster: &ClusterHealthMonitoring) -> HealthMonitoringInfo {
    let nodes: Vec<ClusterNodeInfo> = cluster
        .nodes
        .iter()
        .map(|node| ClusterNodeInfo {
            id: node.id.clone(),
            name: node.name.clone(),
            status: node.status.clone(),
            cpu_percent: node.cpu.utilization * 100.0,
            memory_percent: if node.memory.total > 0.0 {
                (node.memory.used / node.memory.total * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            },
            uptime_seconds: node.uptime.max(0.0) as u64,
        })
        .collect();
    let used: f64 = cluster.nodes.iter().map(|n| n.memory.used).sum();
    let total: f64 = cluster.nodes.iter().map(|n| n.memory.total).sum();
    HealthMonitoringInfo {
        system_name: "Proxmox VE".into(),
        version: String::new(),
        cpu_percent: if nodes.is_empty() {
            0.0
        } else {
            nodes.iter().map(|n| n.cpu_percent).sum::<f64>() / nodes.len() as f64
        },
        cpu_temp_c: None,
        memory_used_bytes: used.max(0.0) as u64,
        memory_total_bytes: total.max(0.0) as u64,
        uptime_seconds: nodes.iter().map(|n| n.uptime_seconds).max().unwrap_or(0),
        load_average: None,
        disks: cluster
            .storages
            .iter()
            .map(|s| HealthMonitoringDisk {
                name: s.name.clone(),
                mount_point: format!("{}:{}", s.node, s.name),
                used_bytes: s.used.max(0.0) as u64,
                total_bytes: s.total.max(0.0) as u64,
            })
            .collect(),
        nodes,
    }
}
