//! Glances (`glances/glances-integration.ts`): system health from the v4 REST API.

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, f64_of, str_of};
use crate::capabilities::health_monitoring::{
    CpuCore, FileSystemInfo, GpuInfo, NetworkThroughput, SmartInfo, SmartStatusReason,
    SystemHealthMonitoring,
};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Serialize, Deserialize)]
struct GlancesSession {
    version: String,
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let req = http::get(rt, ctx, "/api/4/status", &[])?.header("User-Agent", "Homarr");
    http::send(ctx, req).await.map(|_| ())
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let version = match rt.sessions.get::<GlancesSession>(ctx) {
        Some(session) => session.version,
        None => {
            let version = http::send_text(ctx, http::get(rt, ctx, "/api/4/version", &[])?).await?;
            rt.sessions.set(
                ctx,
                &GlancesSession {
                    version: version.clone(),
                },
                None,
            );
            version
        }
    };

    let (stats, cpu_temp, smart) = tokio::join!(
        http::send_json(ctx, http::get(rt, ctx, "/api/4/all", &[])?, "stats"),
        cpu_temp(rt, ctx),
        smart(rt, ctx),
    );
    map_stats(ctx, &stats?, version, cpu_temp, smart)
}

/// CPU temperature is optional: any failure yields `None`.
async fn cpu_temp(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> Option<f64> {
    let request = http::get(rt, ctx, "/api/4/sensors", &[]).ok()?;
    let sensors = http::send_json(ctx, request, "sensors").await.ok()?;
    parse_cpu_temp(&sensors)
}

/// The SMART plugin is disabled by default (Glances answers 400): any failure yields no devices.
async fn smart(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> Vec<SmartInfo> {
    let Ok(request) = http::get(rt, ctx, "/api/4/smart", &[]) else {
        return Vec::new();
    };
    match http::send_json(ctx, request, "smart").await {
        Ok(devices) => map_smart(&devices),
        Err(_) => Vec::new(),
    }
}

fn map_stats(
    ctx: &IntegrationContext,
    stats: &Value,
    version: String,
    cpu_temp: Option<f64>,
    smart: Vec<SmartInfo>,
) -> HomarrResult<SystemHealthMonitoring> {
    let bad = |what: &str| parse_error(ctx, "Glances stats", format!("missing {what}"));
    let cpu_total = stats
        .get("cpu")
        .and_then(|c| f64_of(c, "total"))
        .ok_or_else(|| bad("cpu.total"))?;
    let mem = stats.get("mem").ok_or_else(|| bad("mem"))?;
    let mem_total = f64_of(mem, "total").ok_or_else(|| bad("mem.total"))?;
    let mem_used = f64_of(mem, "used").ok_or_else(|| bad("mem.used"))?;
    let network = stats
        .get("network")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("network"))?;
    let fs = stats
        .get("fs")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("fs"))?;
    let uptime = stats
        .get("uptime")
        .and_then(Value::as_str)
        .and_then(parse_uptime)
        .ok_or_else(|| bad("uptime"))?;

    // `percpu` payload drift must not fail the whole call.
    let cpu_cores: Vec<CpuCore> = stats
        .get("percpu")
        .and_then(Value::as_array)
        .map(|cores| {
            cores
                .iter()
                .filter_map(|core| {
                    Some(CpuCore {
                        id: core.get("cpu_number")?.as_u64()? as u32,
                        utilization: f64_of(core, "total")?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let file_system = fs
        .iter()
        .map(|entry| {
            Some(FileSystemInfo {
                device_name: str_of(entry, "device_name")?.to_string(),
                used: format!("{}", f64_of(entry, "used")?),
                available: format!("{}", f64_of(entry, "free")?),
                size: f64_of(entry, "size"),
                percentage: f64_of(entry, "percent")?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| bad("fs entry"))?;

    let gpu = stats
        .get("gpu")
        .and_then(Value::as_array)
        .map(|gpus| {
            gpus.iter()
                .filter_map(|gpu| {
                    Some(GpuInfo {
                        gpu_id: str_of(gpu, "gpu_id")?.to_string(),
                        name: str_of(gpu, "name")?.to_string(),
                        memory_utilization: f64_of(gpu, "mem").unwrap_or(0.0),
                        processor_utilization: f64_of(gpu, "proc").unwrap_or(0.0),
                        temperature: f64_of(gpu, "temperature"),
                        fan_speed: f64_of(gpu, "fan_speed"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let sum = |key: &str| -> f64 {
        network
            .iter()
            .filter_map(|net| f64_of(net, key))
            .sum::<f64>()
    };

    Ok(SystemHealthMonitoring {
        version,
        cpu_model_name: stats
            .get("quicklook")
            .and_then(|q| str_of(q, "cpu_name"))
            .unwrap_or("Unknown")
            .to_string(),
        cpu_utilization: cpu_total,
        cpu_cores: Some(cpu_cores),
        mem_used_in_bytes: mem_used,
        mem_available_in_bytes: mem_total - mem_used,
        uptime,
        network: Some(NetworkThroughput {
            up: sum("bytes_sent_rate_per_sec"),
            down: sum("bytes_recv_rate_per_sec"),
        }),
        load_average: None,
        reboot_required: false,
        available_pkg_updates: 0,
        cpu_temp,
        file_system,
        smart,
        gpu,
    })
}

/// Python's `str(timedelta)` format: `71 days, 9:51:35`, `1 day, 9:50:23`, `9:51:24`.
pub fn parse_uptime(value: &str) -> Option<f64> {
    let value = value.trim();
    let (days, clock) = match value.split_once(", ") {
        Some((days, clock)) => {
            let (count, unit) = days.split_once(' ')?;
            if unit != "day" && unit != "days" {
                return None;
            }
            (count.parse::<f64>().ok()?, clock)
        }
        None => (0.0, value),
    };
    let mut parts = clock.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(days * 86_400.0 + hours * 3_600.0 + minutes * 60.0 + seconds)
}

const CPU_TEMP_LABEL_PRIORITY: [&str; 2] = ["CPU", "Package id 0"];

pub fn parse_cpu_temp(sensors: &Value) -> Option<f64> {
    let temperature_sensors: Vec<(&str, f64)> = sensors
        .as_array()?
        .iter()
        .filter(|s| matches!(str_of(s, "type"), Some("temperature_core" | "temperature")))
        .filter_map(|s| Some((str_of(s, "label")?, s.get("value")?.as_f64()?)))
        .collect();
    for label in CPU_TEMP_LABEL_PRIORITY {
        if let Some((_, value)) = temperature_sensors.iter().find(|(l, _)| *l == label) {
            return Some(*value);
        }
    }
    temperature_sensors.first().map(|(_, value)| *value)
}

struct SmartAttribute {
    name: Option<String>,
    key: Option<String>,
    value: Option<Value>,
    raw: Option<Value>,
    when_failed: Option<String>,
}

fn nullable_string(value: &Value, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some(Some(s.clone())),
        _ => None,
    }
}

fn nullable_scalar(value: &Value, key: &str) -> Option<Option<Value>> {
    match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v @ (Value::String(_) | Value::Number(_))) => Some(Some(v.clone())),
        _ => None,
    }
}

/// Mirrors `smartAttributeSchema.safeParse`: objects only, with correctly typed optional fields.
fn parse_attribute(value: &Value) -> Option<SmartAttribute> {
    value.as_object()?;
    Some(SmartAttribute {
        name: nullable_string(value, "name")?,
        key: nullable_string(value, "key")?,
        value: nullable_scalar(value, "value")?,
        raw: nullable_scalar(value, "raw")?,
        when_failed: nullable_string(value, "when_failed")?,
    })
}

/// JS `Number(value)` for the string / number scalars Glances reports.
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) if s.trim().is_empty() => 0.0,
        Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// JS `Number.parseInt(String(raw), 10)`: leading integer or `None`.
fn parse_leading_int(value: &Value) -> Option<f64> {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    text[..end].parse::<i64>().ok().map(|n| n as f64)
}

const ATA_TEMPERATURE_NAMES: [&str; 2] = ["Temperature_Celsius", "Airflow_Temperature_Cel"];

fn smart_temperature(attributes: &[SmartAttribute]) -> Option<f64> {
    let nvme = attributes
        .iter()
        .find(|a| a.key.as_deref() == Some("_temperature"))
        .and_then(|a| a.value.as_ref())
        .and_then(|v| v.as_f64().filter(|_| v.is_number()));
    if nvme.is_some() {
        return nvme;
    }
    for name in ATA_TEMPERATURE_NAMES {
        let raw = attributes
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
            .and_then(|a| a.raw.as_ref());
        if let Some(temperature) = raw.and_then(parse_leading_int) {
            return Some(temperature);
        }
    }
    None
}

pub fn map_smart(devices: &Value) -> Vec<SmartInfo> {
    let Some(devices) = devices.as_array() else {
        return Vec::new();
    };
    devices.iter().flat_map(map_smart_device).collect()
}

fn map_smart_device(device: &Value) -> Vec<SmartInfo> {
    let Some(object) = device.as_object() else {
        return Vec::new();
    };
    let Some(device_name) = object.get("DeviceName").and_then(Value::as_str) else {
        return Vec::new();
    };
    let attributes: Vec<SmartAttribute> = object.values().filter_map(parse_attribute).collect();

    // ATA: when_failed is "-" unless the attribute crossed its threshold. NVMe: critical warning bitmask.
    let failed_attributes: Vec<String> = attributes
        .iter()
        .filter(|a| {
            a.when_failed
                .as_deref()
                .is_some_and(|w| !w.is_empty() && w != "-")
        })
        .filter_map(|a| a.name.clone().or_else(|| a.key.clone()))
        .collect();
    let critical_warning = attributes
        .iter()
        .find(|a| a.key.as_deref() == Some("criticalWarning"))
        .and_then(|a| a.value.as_ref())
        .map(js_number)
        .unwrap_or(f64::NAN);
    let is_nvme = critical_warning.is_finite();

    // Without ATA thresholds or an NVMe critical warning there is no verdict, so the widget shows N/A.
    let has_ata_thresholds = attributes.iter().any(|a| a.when_failed.is_some());
    if !has_ata_thresholds && !is_nvme {
        return Vec::new();
    }

    let has_critical_warning = is_nvme && critical_warning != 0.0;
    let healthy = failed_attributes.is_empty() && !has_critical_warning;
    let status_reason = if !failed_attributes.is_empty() {
        SmartStatusReason::AttributesFailed {
            attributes: failed_attributes,
        }
    } else if has_critical_warning {
        SmartStatusReason::CriticalWarning
    } else if is_nvme {
        SmartStatusReason::NoCriticalWarning
    } else {
        SmartStatusReason::AttributesWithinThresholds
    };

    vec![SmartInfo {
        device_name: format!("/dev/{}", device_name.split(' ').next().unwrap_or_default()),
        temperature: smart_temperature(&attributes),
        overall_status: if healthy { "PASSED" } else { "FAILED" }.to_string(),
        healthy,
        status_reason: Some(status_reason),
    }]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_python_uptime_formats() {
        assert_eq!(
            parse_uptime("71 days, 9:51:35"),
            Some(71.0 * 86400.0 + 9.0 * 3600.0 + 51.0 * 60.0 + 35.0)
        );
        assert_eq!(
            parse_uptime("1 day, 9:50:23"),
            Some(86400.0 + 9.0 * 3600.0 + 50.0 * 60.0 + 23.0)
        );
        assert_eq!(parse_uptime("0:00:17"), Some(17.0));
        assert_eq!(parse_uptime("garbage"), None);
    }

    #[test]
    fn cpu_temp_prefers_known_labels() {
        let sensors = json!([
            {"label":"Core 0","value":50.0,"type":"temperature_core"},
            {"label":"Package id 0","value":61.0,"type":"temperature_core"},
            {"label":"CPU","value":70.0,"type":"temperature"},
            {"label":"fan","value":1000.0,"type":"fan_speed"}
        ]);
        assert_eq!(parse_cpu_temp(&sensors), Some(70.0));
        let fallback = json!([{"label":"Core 0","value":50.0,"type":"temperature_core"}]);
        assert_eq!(parse_cpu_temp(&fallback), Some(50.0));
        assert_eq!(
            parse_cpu_temp(&json!([{"label":"x","value":1.0,"type":"fan_speed"}])),
            None
        );
    }

    #[test]
    fn maps_ata_smart_device() {
        let devices = json!([{
            "DeviceName": "sda Samsung",
            "1": {"name":"Raw_Read_Error_Rate","key":"1","value":100,"raw":"0","when_failed":"-"},
            "194": {"name":"Temperature_Celsius","key":"194","value":"34","raw":"35 (Min/Max 20/45)","when_failed":"-"}
        }]);
        let smart = map_smart(&devices);
        assert_eq!(smart.len(), 1);
        assert_eq!(smart[0].device_name, "/dev/sda");
        assert_eq!(smart[0].temperature, Some(35.0));
        assert!(smart[0].healthy);
        assert_eq!(
            smart[0].status_reason,
            Some(SmartStatusReason::AttributesWithinThresholds)
        );
    }

    #[test]
    fn maps_failed_ata_and_nvme_warning() {
        let failed = map_smart(&json!([{
            "DeviceName": "sdb",
            "5": {"name":"Reallocated_Sector_Ct","key":"5","when_failed":"FAILING_NOW"}
        }]));
        assert!(!failed[0].healthy);
        assert_eq!(
            failed[0].status_reason,
            Some(SmartStatusReason::AttributesFailed {
                attributes: vec!["Reallocated_Sector_Ct".into()]
            })
        );
        let nvme = map_smart(&json!([{
            "DeviceName": "nvme0n1 WD",
            "criticalWarning": {"key":"criticalWarning","value":0},
            "_temperature": {"key":"_temperature","value":41}
        }]));
        assert!(nvme[0].healthy);
        assert_eq!(nvme[0].temperature, Some(41.0));
        assert_eq!(
            nvme[0].status_reason,
            Some(SmartStatusReason::NoCriticalWarning)
        );
    }

    #[test]
    fn skips_devices_without_verdict() {
        assert!(map_smart(&json!([{"DeviceName":"sdc"}])).is_empty());
    }
}
