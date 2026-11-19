//! Synology DSM (`synology/synology-client.ts` + `synology-integration.ts`): web API with cookie sessions.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use homarr_common::HomarrResult;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, cookie_header, str_of};
use crate::capabilities::health_monitoring::{
    FileSystemInfo, LoadAverage, NetworkThroughput, SmartInfo, SystemHealthMonitoring,
};
use crate::error::{parse_error, status_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const REQUEST_TIMEOUT_SECS: u64 = 5;
const AUTH_API_NAME: &str = "SYNO.API.Auth";
const INFO_API_NAME: &str = "SYNO.API.Info";
const SESSION_ERROR_CODES: [i64; 3] = [106, 107, 119];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    #[serde(rename = "cookieHeader")]
    cookie_header: String,
}

#[derive(Debug, Clone)]
struct ApiDefinition {
    cgi_path: String,
    max_version: i64,
}

/// API discovery results never change for a given DSM, so they are shared between calls.
fn api_definitions() -> &'static Mutex<HashMap<String, ApiDefinition>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ApiDefinition>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

#[derive(Debug, Clone, PartialEq)]
pub struct VolumeRecord {
    pub name: String,
    pub display_name: Option<String>,
    pub used_bytes: f64,
    pub total_bytes: f64,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskRecord {
    pub identifier: String,
    pub name: String,
    pub status: Option<String>,
    pub temperature: Option<f64>,
    pub volume_name: Option<String>,
}

/// JS `Number.parseFloat` semantics for DSM's number-or-string fields.
pub fn parse_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let s = s.trim_start();
            let end = (0..=s.len())
                .rev()
                .filter(|i| s.is_char_boundary(*i))
                .find(|i| s[..*i].parse::<f64>().is_ok())?;
            s[..end].parse::<f64>().ok().filter(|n| n.is_finite())
        }
        _ => None,
    }
}

fn number_at(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(parse_number)
}

fn api_url(
    ctx: &IntegrationContext,
    definition: &ApiDefinition,
    query: &[(&str, &str)],
) -> HomarrResult<url::Url> {
    let cgi_path = definition.cgi_path.trim();
    if cgi_path.is_empty()
        || cgi_path.starts_with('/')
        || cgi_path.contains('\\')
        || cgi_path
            .split('/')
            .any(|seg| matches!(seg, "" | "." | ".."))
    {
        return Err(parse_error(ctx, "API info", "invalid Synology CGI path"));
    }
    http::url(ctx, &format!("/webapi/{cgi_path}"), query)
}

async fn api_definition(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    api_name: &str,
) -> HomarrResult<ApiDefinition> {
    let cache_key = format!("{}:{api_name}", ctx.id);
    if let Some(definition) = api_definitions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&cache_key)
    {
        return Ok(definition.clone());
    }
    let req = http::get(
        rt,
        ctx,
        "/webapi/query.cgi",
        &[
            ("api", INFO_API_NAME),
            ("version", "1"),
            ("method", "query"),
            ("query", api_name),
        ],
    )?
    .header("User-Agent", "Homarr")
    .timeout(http::timeout(REQUEST_TIMEOUT_SECS));
    let payload = http::send_json(ctx, req, "API info").await?;
    let definition = payload
        .get("data")
        .and_then(|d| d.get(api_name))
        .filter(|_| payload.get("success").and_then(Value::as_bool) == Some(true))
        .and_then(|d| {
            Some(ApiDefinition {
                cgi_path: str_of(d, "path")?.to_string(),
                max_version: d.get("maxVersion")?.as_i64()?,
            })
        })
        .ok_or_else(|| status_error(ctx, reqwest::StatusCode::BAD_GATEWAY))?;
    api_definitions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cache_key, definition.clone());
    Ok(definition)
}

async fn login(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<StoredSession> {
    let definition = api_definition(rt, ctx, AUTH_API_NAME).await?;
    let version = definition.max_version.to_string();
    let url = api_url(ctx, &definition, &[])?;
    let req = rt
        .http
        .standard()
        .request(Method::POST, url)
        .header("User-Agent", "Homarr")
        .timeout(http::timeout(REQUEST_TIMEOUT_SECS))
        .form(&[
            ("api", AUTH_API_NAME),
            ("version", version.as_str()),
            ("method", "login"),
            ("account", ctx.require_secret("username")?),
            ("passwd", ctx.require_secret("password")?),
            ("format", "cookie"),
        ]);
    let response = http::send(ctx, req).await?;
    let cookie = cookie_header(&response);
    let payload: Value = response
        .json()
        .await
        .map_err(|err| crate::error::response_body_error(ctx, "login response", err))?;
    if payload.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(status_error(ctx, reqwest::StatusCode::UNAUTHORIZED));
    }
    let cookie_header =
        cookie.ok_or_else(|| status_error(ctx, reqwest::StatusCode::UNAUTHORIZED))?;
    Ok(StoredSession { cookie_header })
}

enum Attempt {
    Done(Value),
    SessionExpired(i64),
}

async fn request_once(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    session: &StoredSession,
    api_name: &str,
    method: &str,
    extra: &[(&str, &str)],
) -> HomarrResult<Attempt> {
    let definition = api_definition(rt, ctx, api_name).await?;
    let version = definition.max_version.to_string();
    let mut query = vec![
        ("api", api_name),
        ("version", version.as_str()),
        ("method", method),
    ];
    query.extend_from_slice(extra);
    let req = rt
        .http
        .standard()
        .get(api_url(ctx, &definition, &query)?)
        .header("Cookie", &session.cookie_header)
        .header("User-Agent", "Homarr")
        .timeout(http::timeout(REQUEST_TIMEOUT_SECS));
    let payload = http::send_json(ctx, req, api_name).await?;
    match payload.get("success").and_then(Value::as_bool) {
        Some(true) => Ok(Attempt::Done(
            payload.get("data").cloned().unwrap_or(Value::Null),
        )),
        Some(false) => {
            let code = payload.pointer("/error/code").and_then(Value::as_i64);
            match code {
                Some(code) if SESSION_ERROR_CODES.contains(&code) => {
                    Ok(Attempt::SessionExpired(code))
                }
                _ => Err(status_error(ctx, reqwest::StatusCode::BAD_GATEWAY)),
            }
        }
        None => Err(parse_error(ctx, api_name, "missing success flag")),
    }
}

/// Runs an API call with the stored session, logging in again when DSM reports an expired session.
async fn request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    api_name: &str,
    method: &str,
    extra: &[(&str, &str)],
) -> HomarrResult<Value> {
    if let Some(session) = rt.sessions.get::<StoredSession>(ctx) {
        match request_once(rt, ctx, &session, api_name, method, extra).await? {
            Attempt::Done(data) => return Ok(data),
            Attempt::SessionExpired(code) => {
                tracing::debug!(integration = %ctx.name, code, "Synology session expired, re-authenticating");
                rt.sessions.clear(ctx);
            }
        }
    }
    let session = login(rt, ctx).await?;
    rt.sessions.set(ctx, &session, None);
    match request_once(rt, ctx, &session, api_name, method, extra).await? {
        Attempt::Done(data) => Ok(data),
        Attempt::SessionExpired(_) => Err(status_error(ctx, reqwest::StatusCode::UNAUTHORIZED)),
    }
}

async fn request_optional(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    api_name: &str,
    method: &str,
) -> Option<Value> {
    match request(rt, ctx, api_name, method, &[]).await {
        Ok(data) => Some(data),
        Err(error) => {
            tracing::debug!(integration = %ctx.name, api_name, %error, "Optional Synology API request failed");
            None
        }
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    request(rt, ctx, "SYNO.Core.System", "info", &[])
        .await
        .map(|_| ())
}

fn map_volume(
    name: &str,
    used: Option<&Value>,
    total: Option<&Value>,
    status: Option<&str>,
    display_name: Option<&str>,
) -> Option<VolumeRecord> {
    let used_bytes = parse_number(used?)?;
    let total_bytes = parse_number(total?)?;
    if total_bytes <= 0.0 {
        return None;
    }
    Some(VolumeRecord {
        name: name.to_string(),
        display_name: display_name.filter(|d| *d != name).map(str::to_string),
        used_bytes,
        total_bytes,
        status: status.map(str::to_string),
    })
}

fn map_legacy_volumes(volumes: Option<&Value>) -> Vec<VolumeRecord> {
    volumes
        .and_then(Value::as_array)
        .map(|volumes| {
            volumes
                .iter()
                .filter_map(|v| {
                    map_volume(
                        str_of(v, "name")?,
                        v.get("used_size"),
                        v.get("total_size"),
                        str_of(v, "status"),
                        None,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn map_storage_v2_volumes(storage: &Value) -> Vec<VolumeRecord> {
    storage
        .get("volumes")
        .and_then(Value::as_array)
        .map(|volumes| {
            volumes
                .iter()
                .filter_map(|v| {
                    let name = str_of(v, "id")
                        .or_else(|| str_of(v, "vol_path"))
                        .or_else(|| str_of(v, "display_name"))?;
                    map_volume(
                        name,
                        v.pointer("/size/used"),
                        v.pointer("/size/total"),
                        str_of(v, "status"),
                        str_of(v, "display_name"),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Volumes from `storage_v2`, falling back to the legacy `storage` info.
pub async fn storage_volumes(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<VolumeRecord>> {
    match request(
        rt,
        ctx,
        "SYNO.Core.System",
        "info",
        &[("type", "storage_v2")],
    )
    .await
    {
        Ok(storage) => {
            let volumes = map_storage_v2_volumes(&storage);
            if !volumes.is_empty() {
                return Ok(volumes);
            }
            let legacy = map_legacy_volumes(storage.get("vol_info"));
            if !legacy.is_empty() {
                return Ok(legacy);
            }
        }
        Err(error) => {
            tracing::debug!(integration = %ctx.name, %error, "storage_v2 unavailable, falling back to legacy storage info");
        }
    }
    let legacy = request(rt, ctx, "SYNO.Core.System", "info", &[("type", "storage")]).await?;
    Ok(map_legacy_volumes(legacy.get("vol_info")))
}

/// `{ value: "<integration id>:<volume>", label }` entries for the storage widget's volume picker.
pub async fn list_storage_volumes(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<(String, String)>> {
    let volumes = storage_volumes(rt, ctx).await?;
    Ok(volumes
        .into_iter()
        .map(|v| {
            (
                format!("{}:{}", ctx.id, v.name),
                format!(
                    "{} ({})",
                    v.display_name.as_deref().unwrap_or(&v.name),
                    ctx.name
                ),
            )
        })
        .collect())
}

pub fn map_disks_from_load_info(load_info: &Value) -> Vec<DiskRecord> {
    load_info
        .get("disks")
        .and_then(Value::as_array)
        .map(|disks| {
            disks
                .iter()
                .map(|d| DiskRecord {
                    identifier: first_str(d, &["id", "name", "display_name"])
                        .unwrap_or("unknown-disk")
                        .to_string(),
                    name: first_str(d, &["display_name", "name", "id"])
                        .unwrap_or("unknown-disk")
                        .to_string(),
                    status: first_str(d, &["smart_status", "status"]).map(str::to_string),
                    temperature: d
                        .get("temp")
                        .and_then(parse_number)
                        .or_else(|| number_at(d, "temperature")),
                    volume_name: first_str(d, &["volume_id", "vol_path"]).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn map_disks_from_smart_info(smart_info: &Value) -> Vec<DiskRecord> {
    smart_info
        .get("disks")
        .or_else(|| smart_info.get("items"))
        .and_then(Value::as_array)
        .map(|disks| {
            disks
                .iter()
                .map(|d| DiskRecord {
                    identifier: first_str(d, &["disk_id", "id", "name"])
                        .unwrap_or("unknown-disk")
                        .to_string(),
                    name: first_str(d, &["name", "disk_id", "id"])
                        .unwrap_or("unknown-disk")
                        .to_string(),
                    status: first_str(d, &["overall_status", "status"]).map(str::to_string),
                    temperature: d
                        .get("temp")
                        .and_then(parse_number)
                        .or_else(|| number_at(d, "temperature")),
                    volume_name: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn first_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| str_of(value, k))
}

pub fn merge_disk_records(load: Vec<DiskRecord>, smart: Vec<DiskRecord>) -> Vec<DiskRecord> {
    let mut merged: Vec<DiskRecord> = Vec::new();
    for disk in load {
        match merged.iter().position(|d| d.identifier == disk.identifier) {
            Some(i) => merged[i] = disk,
            None => merged.push(disk),
        }
    }
    for disk in smart {
        match merged.iter().position(|d| d.identifier == disk.identifier) {
            Some(i) => {
                let existing = merged[i].clone();
                merged[i] = DiskRecord {
                    identifier: disk.identifier,
                    name: existing.name,
                    status: disk.status.or(existing.status),
                    temperature: disk.temperature.or(existing.temperature),
                    volume_name: existing.volume_name.or(disk.volume_name),
                };
            }
            None => merged.push(disk),
        }
    }
    merged
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let (system_info, utilization, volumes) = tokio::join!(
        request(rt, ctx, "SYNO.Core.System", "info", &[]),
        request(rt, ctx, "SYNO.Core.System.Utilization", "get", &[]),
        storage_volumes(rt, ctx),
    );
    let (system_info, utilization, volumes) = (system_info?, utilization?, volumes?);
    let (load_info, smart_info, system_status, upgrade_status, upgrade_check) = tokio::join!(
        request_optional(rt, ctx, "SYNO.Storage.CGI.Storage", "load_info"),
        request_optional(rt, ctx, "SYNO.Storage.CGI.Smart", "get_smart_info"),
        request_optional(rt, ctx, "SYNO.Core.System.Status", "get"),
        request_optional(rt, ctx, "SYNO.Core.Upgrade", "status"),
        request_optional(rt, ctx, "SYNO.Core.Upgrade.Server", "check"),
    );
    let disks = merge_disk_records(
        load_info
            .as_ref()
            .map(map_disks_from_load_info)
            .unwrap_or_default(),
        smart_info
            .as_ref()
            .map(map_disks_from_smart_info)
            .unwrap_or_default(),
    );
    let reboot_required = system_status
        .as_ref()
        .and_then(|s| s.get("reboot_required").and_then(Value::as_bool))
        .or_else(|| {
            upgrade_status
                .as_ref()
                .and_then(|s| s.get("reboot_required").and_then(Value::as_bool))
        })
        .unwrap_or(false);
    let available_pkg_updates = upgrade_check
        .as_ref()
        .map(
            |check| match check.get("update_count").and_then(Value::as_u64) {
                Some(count) => count as u32,
                None => u32::from(check.get("available").and_then(Value::as_bool) == Some(true)),
            },
        )
        .unwrap_or(0);
    let (mem_used, mem_available) = map_memory_bytes(&utilization);
    Ok(SystemHealthMonitoring {
        version: map_version(&system_info),
        cpu_model_name: str_of(&system_info, "model")
            .unwrap_or("Unknown CPU")
            .to_string(),
        cpu_utilization: map_cpu_utilization(&utilization),
        cpu_cores: None,
        mem_used_in_bytes: mem_used,
        mem_available_in_bytes: mem_available,
        uptime: map_uptime_seconds(&system_info),
        network: map_network_traffic(&utilization),
        load_average: map_load_average(&utilization),
        reboot_required,
        available_pkg_updates,
        cpu_temp: map_cpu_temperature(&system_info),
        file_system: map_file_system_entries(&volumes),
        smart: map_volume_smart_entries(&volumes, &disks),
        gpu: Vec::new(),
    })
}

pub fn map_version(system_info: &Value) -> String {
    if let Some(v) = str_of(system_info, "version_string").filter(|s| !s.is_empty()) {
        return v.to_string();
    }
    if let Some(v) = str_of(system_info, "firmware_ver").filter(|s| !s.is_empty()) {
        return v.to_string();
    }
    match system_info.get("version") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "Unknown".to_string(),
    }
}

pub fn map_cpu_utilization(utilization: &Value) -> f64 {
    let cpu = utilization.get("cpu");
    let user = cpu.and_then(|c| number_at(c, "user_load")).unwrap_or(0.0);
    let system = cpu.and_then(|c| number_at(c, "system_load")).unwrap_or(0.0);
    (user + system).min(100.0)
}

/// DSM reports memory sizes in KiB. Returns `(used, available)` in bytes.
pub fn map_memory_bytes(utilization: &Value) -> (f64, f64) {
    let memory = utilization.get("memory");
    let total_real = memory.and_then(|m| number_at(m, "total_real"));
    let real_usage = memory.and_then(|m| number_at(m, "real_usage"));
    if let (Some(total_kib), Some(usage)) = (total_real, real_usage) {
        let total_bytes = total_kib * 1024.0;
        let used = (total_bytes * usage / 100.0).round();
        return (used, (total_bytes - used).max(0.0));
    }
    let available = memory
        .and_then(|m| number_at(m, "avail_real"))
        .unwrap_or(0.0);
    let cached = memory.and_then(|m| number_at(m, "cached")).unwrap_or(0.0);
    let total = total_real.unwrap_or(available + cached);
    let used = (total - available).max(0.0);
    (used * 1024.0, available * 1024.0)
}

pub fn map_network_traffic(utilization: &Value) -> Option<NetworkThroughput> {
    let total = utilization
        .get("network")?
        .as_array()?
        .iter()
        .find(|n| str_of(n, "device") == Some("total"))?;
    Some(NetworkThroughput {
        up: number_at(total, "tx")?,
        down: number_at(total, "rx")?,
    })
}

pub fn map_load_average(utilization: &Value) -> Option<LoadAverage> {
    let cpu = utilization.get("cpu")?;
    let (one, five, fifteen) = (
        number_at(cpu, "1min_load"),
        number_at(cpu, "5min_load"),
        number_at(cpu, "15min_load"),
    );
    if one.is_none() && five.is_none() && fifteen.is_none() {
        return None;
    }
    Some(LoadAverage {
        one_min: one.unwrap_or(0.0),
        five_min: five.unwrap_or(0.0),
        fifteen_min: fifteen.unwrap_or(0.0),
    })
}

pub fn map_uptime_seconds(system_info: &Value) -> f64 {
    if let Some(uptime) = system_info.get("uptime").and_then(Value::as_f64) {
        return uptime;
    }
    let Some(up_time) = str_of(system_info, "up_time").filter(|s| !s.is_empty()) else {
        return 0.0;
    };
    let parts: Vec<Option<i64>> = up_time
        .split(':')
        .map(|p| {
            let digits: String = p.trim().chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect();
    match parts.as_slice() {
        [Some(h), Some(m), Some(s)] => (h * 3600 + m * 60 + s) as f64,
        _ => 0.0,
    }
}

pub fn map_cpu_temperature(system_info: &Value) -> Option<f64> {
    system_info
        .get("sys_temp")
        .or_else(|| system_info.get("temperature"))
        .and_then(parse_number)
}

pub fn map_file_system_entries(volumes: &[VolumeRecord]) -> Vec<FileSystemInfo> {
    volumes
        .iter()
        .map(|v| FileSystemInfo {
            device_name: v.name.clone(),
            used: format!("{}", v.used_bytes),
            available: format!("{}", (v.total_bytes - v.used_bytes).max(0.0)),
            size: None,
            percentage: v.used_bytes / v.total_bytes * 100.0,
        })
        .collect()
}

const UNHEALTHY_STATUS_TOKENS: [&str; 7] = [
    "critical", "error", "crash", "failed", "fail", "degraded", "warning",
];

fn status_severity_index(status: &str) -> usize {
    let normalized = status.to_lowercase();
    UNHEALTHY_STATUS_TOKENS
        .iter()
        .position(|t| normalized.contains(t))
        .unwrap_or(UNHEALTHY_STATUS_TOKENS.len())
}

fn pick_worst_disk_status(disks: &[&DiskRecord]) -> String {
    disks
        .iter()
        .filter_map(|d| d.status.as_deref().filter(|s| !s.is_empty()))
        .reduce(|worst, current| {
            if status_severity_index(current) < status_severity_index(worst) {
                current
            } else {
                worst
            }
        })
        .unwrap_or("normal")
        .to_string()
}

pub fn is_healthy_status(status: Option<&str>) -> bool {
    let Some(status) = status.filter(|s| !s.is_empty()) else {
        return true;
    };
    let normalized = status.to_lowercase();
    ![
        "crash", "degraded", "fail", "failed", "warning", "error", "critical",
    ]
    .iter()
    .any(|t| normalized.contains(t))
}

fn disk_belongs_to_volume(disk: &DiskRecord, volume_name: &str) -> bool {
    let Some(disk_volume) = disk.volume_name.as_deref().filter(|v| !v.is_empty()) else {
        return false;
    };
    disk_volume.contains(volume_name) || volume_name.contains(disk_volume)
}

pub fn map_volume_smart_entries(volumes: &[VolumeRecord], disks: &[DiskRecord]) -> Vec<SmartInfo> {
    volumes
        .iter()
        .map(|volume| {
            let matching: Vec<&DiskRecord> = disks
                .iter()
                .filter(|d| disk_belongs_to_volume(d, &volume.name))
                .collect();
            let temperature = matching
                .iter()
                .filter_map(|d| d.temperature)
                .reduce(f64::max);
            let overall_status = volume
                .status
                .clone()
                .unwrap_or_else(|| pick_worst_disk_status(&matching));
            SmartInfo {
                device_name: volume.name.clone(),
                healthy: is_healthy_status(Some(&overall_status)),
                overall_status,
                temperature,
                status_reason: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_numbers_like_parse_float() {
        assert_eq!(parse_number(&json!("12.5abc")), Some(12.5));
        assert_eq!(parse_number(&json!(3)), Some(3.0));
        assert_eq!(parse_number(&json!("abc")), None);
        assert_eq!(parse_number(&json!(null)), None);
    }

    #[test]
    fn maps_memory_from_usage_percentage() {
        let utilization = json!({"memory": {"real_usage": 35, "total_real": 8388608, "avail_real": 1, "cached": 900000}});
        let (used, available) = map_memory_bytes(&utilization);
        let total: f64 = 8_388_608.0 * 1024.0;
        assert_eq!(used, (total * 0.35).round());
        assert_eq!(available, total - used);
    }

    #[test]
    fn maps_memory_fallback_without_usage() {
        let (used, available) =
            map_memory_bytes(&json!({"memory": {"avail_real": 100, "cached": 20}}));
        assert_eq!((used, available), (20.0 * 1024.0, 100.0 * 1024.0));
    }

    #[test]
    fn maps_cpu_network_and_load() {
        let utilization = json!({
            "cpu": {"user_load": 12, "system_load": "8", "1min_load": 10, "5min_load": 14},
            "network": [{"device":"eth0","rx":1,"tx":2},{"device":"total","rx":4096,"tx":8192}]
        });
        assert_eq!(map_cpu_utilization(&utilization), 20.0);
        let network = map_network_traffic(&utilization).unwrap();
        assert_eq!((network.up, network.down), (8192.0, 4096.0));
        let load = map_load_average(&utilization).unwrap();
        assert_eq!(
            (load.one_min, load.five_min, load.fifteen_min),
            (10.0, 14.0, 0.0)
        );
        assert!(map_load_average(&json!({"cpu": {}})).is_none());
        assert_eq!(
            map_cpu_utilization(&json!({"cpu": {"user_load": 80, "system_load": 40}})),
            100.0
        );
    }

    #[test]
    fn maps_system_info_fields() {
        let info = json!({"up_time": "48:30:15", "version_string": "DSM 7.3", "sys_temp": "42"});
        assert_eq!(map_version(&info), "DSM 7.3");
        assert_eq!(
            map_uptime_seconds(&info),
            48.0 * 3600.0 + 30.0 * 60.0 + 15.0
        );
        assert_eq!(map_cpu_temperature(&info), Some(42.0));
        assert_eq!(map_version(&json!({"version": 7})), "7");
        assert_eq!(map_version(&json!({})), "Unknown");
        assert_eq!(map_uptime_seconds(&json!({"uptime": 99})), 99.0);
    }

    #[test]
    fn maps_storage_v2_and_legacy_volumes() {
        let v2 = json!({"volumes": [
            {"id":"volume_1","display_name":"Data Volume 1","size":{"total":"1000","used":400},"status":"normal"},
            {"id":"volume_0","size":{"total":0,"used":0}}
        ]});
        let volumes = map_storage_v2_volumes(&v2);
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].display_name.as_deref(), Some("Data Volume 1"));
        let entries = map_file_system_entries(&volumes);
        assert_eq!(entries[0].percentage, 40.0);
        assert_eq!(entries[0].available, "600");
        let legacy = map_legacy_volumes(Some(
            &json!([{"name":"/volume1","used_size":"5","total_size":"10"}]),
        ));
        assert_eq!(legacy[0].name, "/volume1");
    }

    #[test]
    fn merges_disks_and_maps_volume_smart() {
        let load = map_disks_from_load_info(&json!({"disks":[
            {"id":"sda","display_name":"Drive 1","temp":38,"status":"normal","volume_id":"volume_1"},
            {"id":"sdb","display_name":"Drive 2","temp":44,"status":"normal","volume_id":"volume_1"}
        ]}));
        let smart = map_disks_from_smart_info(
            &json!({"disks":[{"disk_id":"sdb","overall_status":"warning","temp":"46"}]}),
        );
        let disks = merge_disk_records(load, smart);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[1].status.as_deref(), Some("warning"));
        assert_eq!(disks[1].name, "Drive 2");
        assert_eq!(disks[1].volume_name.as_deref(), Some("volume_1"));
        let volumes = vec![
            VolumeRecord {
                name: "volume_1".into(),
                display_name: None,
                used_bytes: 1.0,
                total_bytes: 2.0,
                status: None,
            },
            VolumeRecord {
                name: "volume_2".into(),
                display_name: None,
                used_bytes: 1.0,
                total_bytes: 2.0,
                status: Some("degraded".into()),
            },
        ];
        let entries = map_volume_smart_entries(&volumes, &disks);
        assert_eq!(entries[0].overall_status, "warning");
        assert!(!entries[0].healthy);
        assert_eq!(entries[0].temperature, Some(46.0));
        assert!(!entries[1].healthy);
        assert_eq!(entries[1].temperature, None);
        assert_eq!(
            map_volume_smart_entries(&volumes[..1], &[])[0].overall_status,
            "normal"
        );
    }
}
