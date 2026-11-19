use homarr_common::HomarrResult;
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;
use serde_json::Value;

use crate::clients::{get_json, simple_get_test};
use crate::types::{
    ClusterNodeInfo, HealthMonitoringDisk, HealthMonitoringInfo, IntegrationContext, UpsSummary,
};

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    match ctx.kind {
        IntegrationKind::Proxmox => simple_get_test(http, ctx, "/api2/json/version").await,
        IntegrationKind::DashDot => simple_get_test(http, ctx, "/info").await,
        IntegrationKind::Glances => simple_get_test(http, ctx, "/api/4/quicklook").await,
        IntegrationKind::Coolify => simple_get_test(http, ctx, "/api/v1/version").await,
        IntegrationKind::Unraid => simple_get_test(http, ctx, "/").await,
        IntegrationKind::UptimeKuma => {
            if let Some(slug) = ctx.secret("slug") {
                simple_get_test(http, ctx, &format!("/api/status-page/{slug}")).await
            } else {
                simple_get_test(http, ctx, "/metrics").await
            }
        }
        IntegrationKind::Wud => simple_get_test(http, ctx, "/api/containers").await,
        IntegrationKind::Llamacpp => simple_get_test(http, ctx, "/health").await,
        IntegrationKind::PeaNut => simple_get_test(http, ctx, "/api/v1/devices").await,
        IntegrationKind::Beszel => simple_get_test(http, ctx, "/api/health").await,
        IntegrationKind::Patchmon => simple_get_test(http, ctx, "/api/v1/hosts").await,
        IntegrationKind::Synology => {
            simple_get_test(
                http,
                ctx,
                "/webapi/query.cgi?api=SYNO.API.Info&version=1&method=query",
            )
            .await
        }
        IntegrationKind::Openmediavault => simple_get_test(http, ctx, "/").await,
        IntegrationKind::Truenas => Ok(()),
        _ => Ok(()),
    }
}

pub async fn get_health_info(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<HealthMonitoringInfo> {
    match ctx.kind {
        IntegrationKind::Proxmox => {
            let val = get_json(http, ctx, "/api2/json/nodes").await?;
            let arr = val
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let nodes: Vec<ClusterNodeInfo> = arr
                .iter()
                .map(|n| {
                    let maxmem = n
                        .get("maxmem")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0)
                        .max(1.0);
                    let mem = n.get("mem").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    ClusterNodeInfo {
                        id: n
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("node")
                            .to_string(),
                        name: n
                            .get("node")
                            .and_then(|v| v.as_str())
                            .unwrap_or("node")
                            .to_string(),
                        status: n
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        cpu_percent: n.get("cpu").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0,
                        memory_percent: (mem / maxmem * 100.0).clamp(0.0, 100.0),
                        uptime_seconds: n.get("uptime").and_then(|v| v.as_u64()).unwrap_or(0),
                    }
                })
                .collect();
            let first_cpu = nodes.first().map(|n| n.cpu_percent).unwrap_or(0.0);
            Ok(HealthMonitoringInfo {
                system_name: "Proxmox VE".into(),
                version: "8.x".into(),
                cpu_percent: first_cpu,
                cpu_temp_c: None,
                memory_used_bytes: 0,
                memory_total_bytes: 0,
                uptime_seconds: nodes.first().map(|n| n.uptime_seconds).unwrap_or(0),
                load_average: None,
                disks: Vec::new(),
                nodes,
            })
        }
        IntegrationKind::Glances => {
            let ql = get_json(http, ctx, "/api/4/quicklook").await?;
            let mem = get_json(http, ctx, "/api/4/mem")
                .await
                .unwrap_or(Value::Null);
            Ok(HealthMonitoringInfo {
                system_name: ql
                    .get("cpu_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Glances Host")
                    .to_string(),
                version: "Glances".into(),
                cpu_percent: ql.get("cpu").and_then(|v| v.as_f64()).unwrap_or(0.0),
                cpu_temp_c: None,
                memory_used_bytes: mem.get("used").and_then(|v| v.as_u64()).unwrap_or(0),
                memory_total_bytes: mem.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
                uptime_seconds: 0,
                load_average: None,
                disks: Vec::new(),
                nodes: Vec::new(),
            })
        }
        IntegrationKind::DashDot => {
            let info = get_json(http, ctx, "/info").await?;
            let cpu_load = get_json(http, ctx, "/load/cpu")
                .await
                .unwrap_or(Value::Null);
            let cpu_pct = cpu_load
                .as_array()
                .and_then(|arr| {
                    if arr.is_empty() {
                        None
                    } else {
                        let sum: f64 = arr
                            .iter()
                            .filter_map(|x| x.get("load").and_then(|l| l.as_f64()))
                            .sum();
                        Some(sum / arr.len() as f64)
                    }
                })
                .unwrap_or(0.0);
            Ok(HealthMonitoringInfo {
                system_name: info
                    .get("os")
                    .and_then(|o| o.get("distro"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Dash.")
                    .to_string(),
                version: "Dash.".into(),
                cpu_percent: cpu_pct,
                cpu_temp_c: None,
                memory_used_bytes: 0,
                memory_total_bytes: info
                    .get("ram")
                    .and_then(|r| r.get("size"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                uptime_seconds: 0,
                load_average: None,
                disks: vec![HealthMonitoringDisk {
                    name: "System".into(),
                    mount_point: "/".into(),
                    used_bytes: 0,
                    total_bytes: 0,
                }],
                nodes: Vec::new(),
            })
        }
        _ => Ok(HealthMonitoringInfo {
            system_name: ctx.name.clone(),
            version: "1.0".into(),
            cpu_percent: 0.0,
            cpu_temp_c: None,
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            uptime_seconds: 0,
            load_average: None,
            disks: Vec::new(),
            nodes: Vec::new(),
        }),
    }
}

pub async fn get_ups_summary(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<UpsSummary>> {
    let val = get_json(http, ctx, "/api/v1/devices").await?;
    let Some(arr) = val.as_array() else {
        return Ok(Vec::new());
    };
    Ok(arr
        .iter()
        .map(|d| UpsSummary {
            name: d
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("UPS")
                .to_string(),
            status: d
                .get("ups.status")
                .and_then(|v| v.as_str())
                .unwrap_or("OL")
                .to_string(),
            battery_charge_percent: d
                .get("battery.charge")
                .and_then(|v| v.as_f64())
                .unwrap_or(100.0),
            load_percent: d.get("ups.load").and_then(|v| v.as_f64()).unwrap_or(0.0),
            runtime_seconds: d
                .get("battery.runtime")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        })
        .collect())
}
