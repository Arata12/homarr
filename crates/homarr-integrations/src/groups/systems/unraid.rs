//! Unraid (`unraid/unraid-integration.ts`): system health through the GraphQL API.

use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde_json::{json, Value};

use super::http::{self, as_f64, f64_of, str_of};
use crate::capabilities::health_monitoring::{
    CpuCore, FileSystemInfo, SmartInfo, SystemHealthMonitoring,
};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const SYSTEM_QUERY: &str = "query { metrics { cpu { percentTotal cpus { percentTotal } } memory { available used free total swapFree swapTotal swapUsed percentTotal } } array { state capacity { disks { free total used } } disks { id name device fsSize fsFree fsUsed status temp } caches { id name device fsSize fsFree fsUsed status temp } } info { devices { network { speed dhcp model } } os { platform distro release uptime } cpu { manufacturer brand cores threads } } }";

async fn query(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    query: &str,
) -> HomarrResult<Value> {
    let req = http::request(rt, ctx, Method::POST, "/graphql", &[])?
        .header("x-api-key", ctx.require_secret("apiKey")?)
        .json(&json!({ "query": query }));
    let response = http::send_json(ctx, req, "GraphQL response").await?;
    if let Some(errors) = response.get("errors").filter(|e| !e.is_null()) {
        let messages: Vec<&str> = errors
            .as_array()
            .map(|errors| errors.iter().filter_map(|e| str_of(e, "message")).collect())
            .unwrap_or_default();
        return Err(HomarrError::Upstream(format!(
            "GraphQL errors from {}: {}",
            ctx.name,
            messages.join(", ")
        )));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| parse_error(ctx, "GraphQL response", "missing data"))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    query(rt, ctx, "query { info { os { platform } } }")
        .await
        .map(|_| ())
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let data = query(rt, ctx, SYSTEM_QUERY).await?;
    map_system_info(ctx, &data, Utc::now())
}

/// Filesystem sizes are reported either as numbers or as numeric strings; `null` when unknown.
fn size_of(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(as_f64).filter(|n| *n >= 0.0)
}

fn parse_boot_time(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc)),
        Value::Number(n) => DateTime::from_timestamp_millis(n.as_f64()? as i64),
        _ => None,
    }
}

pub fn map_system_info(
    ctx: &IntegrationContext,
    data: &Value,
    now: DateTime<Utc>,
) -> HomarrResult<SystemHealthMonitoring> {
    let bad = |what: &str| parse_error(ctx, "Unraid system info", format!("missing {what}"));
    let cpus = data
        .pointer("/metrics/cpu/cpus")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("metrics.cpu.cpus"))?;
    let core_loads: Vec<f64> = cpus
        .iter()
        .map(|c| f64_of(c, "percentTotal").ok_or_else(|| bad("cpu percentTotal")))
        .collect::<Result<_, _>>()?;
    let cpu_utilization = if core_loads.is_empty() {
        0.0
    } else {
        core_loads.iter().sum::<f64>() / core_loads.len() as f64
    };
    let memory = data
        .pointer("/metrics/memory")
        .ok_or_else(|| bad("metrics.memory"))?;
    let total_memory = f64_of(memory, "total").ok_or_else(|| bad("memory.total"))?;
    let available = f64_of(memory, "available").ok_or_else(|| bad("memory.available"))?;
    let used_memory = (total_memory - available).max(0.0);

    let os = data.pointer("/info/os").ok_or_else(|| bad("info.os"))?;
    let boot = os
        .get("uptime")
        .and_then(parse_boot_time)
        .ok_or_else(|| bad("info.os.uptime"))?;
    let cpu_brand = data
        .pointer("/info/cpu/brand")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("info.cpu.brand"))?;

    let array = data.get("array").ok_or_else(|| bad("array"))?;
    let disks: Vec<&Value> = ["disks", "caches"]
        .iter()
        .flat_map(|key| {
            array
                .get(*key)
                .and_then(Value::as_array)
                .map(|a| a.iter().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    let disk_name = |disk: &Value| -> String {
        str_of(disk, "name")
            .or_else(|| str_of(disk, "device"))
            .or_else(|| str_of(disk, "id"))
            .unwrap_or_default()
            .to_string()
    };

    let file_system = disks
        .iter()
        .filter_map(|disk| {
            // Only the pool member reporting filesystem statistics represents its capacity.
            let (size, free, used) = (
                size_of(disk, "fsSize")?,
                size_of(disk, "fsFree")?,
                size_of(disk, "fsUsed")?,
            );
            if size == 0.0 {
                return None;
            }
            Some(FileSystemInfo {
                device_name: disk_name(disk),
                used: format!("{}", used * 1024.0), // API filesystem sizes are in KiB.
                available: format!("{}", free * 1024.0),
                size: None,
                percentage: used / size * 100.0,
            })
        })
        .collect();

    // See ArrayDiskStatus in the Unraid API schema.
    let smart = disks
        .iter()
        .map(|disk| {
            let status = str_of(disk, "status");
            SmartInfo {
                device_name: disk_name(disk),
                temperature: f64_of(disk, "temp"),
                overall_status: status.unwrap_or("UNKNOWN").to_string(),
                healthy: status == Some("DISK_OK"),
                status_reason: None,
            }
        })
        .collect();

    Ok(SystemHealthMonitoring {
        version: str_of(os, "release")
            .ok_or_else(|| bad("os.release"))?
            .to_string(),
        cpu_model_name: cpu_brand.to_string(),
        cpu_utilization,
        cpu_cores: Some(
            core_loads
                .iter()
                .enumerate()
                .map(|(id, utilization)| CpuCore {
                    id: id as u32,
                    utilization: *utilization,
                })
                .collect(),
        ),
        mem_used_in_bytes: used_memory,
        mem_available_in_bytes: total_memory - used_memory,
        uptime: (now - boot).num_seconds() as f64,
        network: None, // See https://github.com/unraid/api/issues/1602
        load_average: None,
        reboot_required: false,
        available_pkg_updates: 0,
        cpu_temp: None, // See https://github.com/unraid/api/issues/1597
        file_system,
        smart,
        gpu: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "u".into(),
            name: "Unraid".into(),
            url: "http://unraid".into(),
            external_url: None,
            kind: IntegrationKind::Unraid,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn maps_disks_and_memory() {
        let data = json!({
            "metrics": {
                "cpu": {"percentTotal": 20.0, "cpus": [{"percentTotal": 10.0}, {"percentTotal": 30.0}]},
                "memory": {"available": 600.0, "used": 400.0, "free": 1.0, "total": 1000.0, "percentTotal": 40.0}
            },
            "array": {
                "state": "STARTED",
                "capacity": {"disks": {"free": "1", "total": "2", "used": "1"}},
                "disks": [
                    {"id":"d1","name":"disk1","device":"sdb","fsSize":"1000","fsFree":600,"fsUsed":400,"status":"DISK_OK","temp":31},
                    {"id":"d2","name":null,"device":"sdc","fsSize":null,"fsFree":null,"fsUsed":null,"status":"DISK_DSBL","temp":null}
                ],
                "caches": []
            },
            "info": {
                "os": {"platform":"linux","distro":"Unraid","release":"7.0","uptime":"2026-01-01T00:00:00Z"},
                "cpu": {"manufacturer":"AMD","brand":"Ryzen","cores":4,"threads":8}
            }
        });
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:01:40Z")
            .unwrap()
            .with_timezone(&Utc);
        let info = map_system_info(&ctx(), &data, now).unwrap();
        assert_eq!(info.cpu_utilization, 20.0);
        assert_eq!(info.mem_used_in_bytes, 400.0);
        assert_eq!(info.uptime, 100.0);
        assert_eq!(info.file_system.len(), 1);
        assert_eq!(info.file_system[0].used, "409600");
        assert_eq!(info.file_system[0].percentage, 40.0);
        assert_eq!(info.smart.len(), 2);
        assert!(info.smart[0].healthy);
        assert!(!info.smart[1].healthy);
        assert_eq!(info.smart[1].device_name, "sdc");
    }
}
