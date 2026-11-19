//! Dash. (`dashdot/dashdot-integration.ts`): system health from the public load endpoints.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use homarr_common::HomarrResult;
use serde::Deserialize;
use serde_json::Value;

use super::http::{self, as_f64};
use crate::capabilities::health_monitoring::{
    CpuCore, FileSystemInfo, GpuInfo, LoadAverage, NetworkThroughput, SystemHealthMonitoring,
};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const HISTORY_LIMIT: usize = 100;
const HISTORY_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Per-core loads of one CPU sample.
type CpuSample = Vec<f64>;

type HistoryMap = HashMap<String, VecDeque<(Instant, CpuSample)>>;

/// In-memory stand-in for the redis `integration:<id>:history:cpu` channel (newest sample first).
fn history() -> &'static Mutex<HistoryMap> {
    static HISTORY: OnceLock<Mutex<HistoryMap>> = OnceLock::new();
    HISTORY.get_or_init(Default::default)
}

fn push_history(id: &str, sample: CpuSample) -> Vec<CpuSample> {
    let mut map = history()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entries = map.entry(id.to_string()).or_default();
    entries.push_front((Instant::now(), sample));
    entries.truncate(HISTORY_LIMIT);
    entries
        .iter()
        .filter(|(at, _)| at.elapsed() <= HISTORY_WINDOW)
        .map(|(_, sample)| sample.clone())
        .collect()
}

fn read_history(id: &str) -> Vec<CpuSample> {
    let map = history()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    map.get(id)
        .map(|entries| {
            entries
                .iter()
                .filter(|(at, _)| at.elapsed() <= HISTORY_WINDOW)
                .map(|(_, sample)| sample.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct Info {
    os: InfoOs,
    cpu: InfoCpu,
    ram: InfoRam,
    storage: Vec<InfoStorage>,
    gpu: InfoGpu,
}

#[derive(Deserialize)]
struct InfoOs {
    distro: String,
    kernel: String,
    release: String,
    uptime: f64,
}

#[derive(Deserialize)]
struct InfoCpu {
    brand: String,
    model: String,
}

#[derive(Deserialize)]
struct InfoRam {
    size: f64,
}

#[derive(Deserialize)]
struct InfoStorage {
    size: f64,
    disks: Vec<InfoDisk>,
}

#[derive(Deserialize)]
struct InfoDisk {
    device: String,
}

#[derive(Deserialize)]
struct InfoGpu {
    #[serde(default)]
    layout: Vec<InfoGpuEntry>,
}

#[derive(Deserialize)]
struct InfoGpuEntry {
    brand: String,
}

#[derive(Deserialize)]
struct CpuCoreLoad {
    load: f64,
    temp: f64,
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    http::send_json(ctx, http::get(rt, ctx, "/info", &[])?, "info")
        .await
        .map(|_| ())
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let (info, cpu, memory, storage, network) = tokio::join!(
        fetch_info(rt, ctx),
        fetch_cpu(rt, ctx),
        fetch_memory(rt, ctx),
        fetch_storage(rt, ctx),
        fetch_network(rt, ctx),
    );
    let info = info?;
    let cpu = cpu?;
    let memory = memory?;
    let storage_load = storage?;
    let network = network?;
    let gpu_load = fetch_gpu_load(rt, ctx).await;

    // Read history after the CPU request has persisted its newest sample.
    let history = read_history(&ctx.id);
    let average = |samples: &[CpuSample]| -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        samples.iter().map(|s| average_of(s)).sum::<f64>() / samples.len() as f64
    };
    let take = |n: usize| &history[..history.len().min(n)];

    let file_system = info
        .storage
        .iter()
        .enumerate()
        .filter_map(|(index, storage)| {
            let used = *storage_load.get(index)?;
            if !used.is_finite() || used < 0.0 || storage.size <= 0.0 {
                return None;
            }
            let devices: Vec<&str> = storage.disks.iter().map(|d| d.device.as_str()).collect();
            Some(FileSystemInfo {
                device_name: format!("Storage {}: ({})", index + 1, devices.join(", ")),
                used: format!("{used}"),
                available: format!("{}", (storage.size - used).max(0.0)),
                size: None,
                percentage: used / storage.size * 100.0,
            })
        })
        .collect();

    Ok(SystemHealthMonitoring {
        version: format!(
            "{} {} ({})",
            info.os.distro, info.os.release, info.os.kernel
        ),
        cpu_model_name: if info.cpu.model.is_empty() {
            format!("Unknown Model ({})", info.cpu.brand)
        } else {
            format!("{} ({})", info.cpu.model, info.cpu.brand)
        },
        cpu_utilization: cpu.sum_load,
        cpu_cores: Some(cpu.cores),
        mem_used_in_bytes: memory,
        mem_available_in_bytes: info.ram.size - memory,
        uptime: info.os.uptime,
        network,
        load_average: Some(LoadAverage {
            one_min: history
                .first()
                .map(|s| average_of(s))
                .unwrap_or(0.0)
                .round(),
            five_min: average(take(4)).round(),
            fifteen_min: average(take(14)).round(),
        }),
        reboot_required: false,
        available_pkg_updates: 0,
        cpu_temp: Some(cpu.average_temperature),
        file_system,
        smart: Vec::new(), // The API does not provide S.M.A.R.T data.
        gpu: gpu_load
            .iter()
            .enumerate()
            .map(|(index, (memory, load))| GpuInfo {
                gpu_id: format!("gpu-{index}"),
                name: info
                    .gpu
                    .layout
                    .get(index)
                    .map(|g| g.brand.clone())
                    .unwrap_or_else(|| format!("GPU {index}")),
                memory_utilization: *memory,
                processor_utilization: *load,
                temperature: None,
                fan_speed: None,
            })
            .collect(),
    })
}

fn average_of(sample: &[f64]) -> f64 {
    if sample.is_empty() {
        0.0
    } else {
        sample.iter().sum::<f64>() / sample.len() as f64
    }
}

async fn fetch_info(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Info> {
    let value = http::send_json(ctx, http::get(rt, ctx, "/info", &[])?, "info").await?;
    serde_json::from_value(value).map_err(|err| parse_error(ctx, "info", err))
}

struct CpuLoad {
    sum_load: f64,
    cores: Vec<CpuCore>,
    average_temperature: f64,
}

/// The endpoint answers with an empty body when the widget is disabled in Dash.
async fn fetch_optional_text(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Option<Value>> {
    let text = http::send_text(ctx, http::get(rt, ctx, path, &[])?).await?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|err| parse_error(ctx, path, err))
}

async fn fetch_cpu(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<CpuLoad> {
    let Some(value) = fetch_optional_text(rt, ctx, "/load/cpu").await? else {
        return Ok(CpuLoad {
            sum_load: 0.0,
            cores: Vec::new(),
            average_temperature: 0.0,
        });
    };
    let data: Vec<CpuCoreLoad> =
        serde_json::from_value(value).map_err(|err| parse_error(ctx, "cpu load", err))?;
    let loads: Vec<f64> = data.iter().map(|c| c.load).collect();
    push_history(&ctx.id, loads.clone());
    Ok(CpuLoad {
        sum_load: average_of(&loads),
        cores: data
            .iter()
            .enumerate()
            .map(|(id, core)| CpuCore {
                id: id as u32,
                utilization: core.load,
            })
            .collect(),
        average_temperature: if data.is_empty() {
            0.0
        } else {
            data.iter().map(|c| c.temp).sum::<f64>() / data.len() as f64
        },
    })
}

async fn fetch_memory(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<f64> {
    let value = http::send_json(ctx, http::get(rt, ctx, "/load/ram", &[])?, "memory load").await?;
    // An empty JSON object means the ram widget is disabled.
    if value.as_object().is_some_and(|o| o.is_empty()) {
        return Ok(0.0);
    }
    http::f64_of(&value, "load").ok_or_else(|| parse_error(ctx, "memory load", "missing load"))
}

async fn fetch_storage(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<f64>> {
    let Some(value) = fetch_optional_text(rt, ctx, "/load/storage").await? else {
        return Ok(Vec::new());
    };
    Ok(value
        .as_array()
        .map(|a| a.iter().map(|v| as_f64(v).unwrap_or(f64::NAN)).collect())
        .unwrap_or_default())
}

async fn fetch_network(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Option<NetworkThroughput>> {
    let Some(value) = fetch_optional_text(rt, ctx, "/load/network").await? else {
        return Ok(None);
    };
    match (http::f64_of(&value, "up"), http::f64_of(&value, "down")) {
        (Some(up), Some(down)) => Ok(Some(NetworkThroughput { up, down })),
        _ => Err(parse_error(ctx, "network load", "missing up/down")),
    }
}

/// Optional: any failure yields no GPUs. Returns `(memory, load)` per GPU.
async fn fetch_gpu_load(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> Vec<(f64, f64)> {
    let Ok(request) = http::get(rt, ctx, "/load/gpu", &[]) else {
        return Vec::new();
    };
    let Ok(value) = http::send_json(ctx, request, "gpu load").await else {
        return Vec::new();
    };
    parse_gpu_layout(&value)
}

pub fn parse_gpu_layout(value: &Value) -> Vec<(f64, f64)> {
    value
        .get("layout")
        .and_then(Value::as_array)
        .map(|layout| {
            layout
                .iter()
                .map(|gpu| {
                    (
                        http::f64_of(gpu, "memory").unwrap_or(0.0),
                        http::f64_of(gpu, "load").unwrap_or(0.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_gpu_layout_with_missing_values() {
        let layout = parse_gpu_layout(&json!({"layout":[{"load":10.0,"memory":20.0},{}]}));
        assert_eq!(layout, vec![(20.0, 10.0), (0.0, 0.0)]);
        assert!(parse_gpu_layout(&json!({})).is_empty());
    }

    #[test]
    fn history_keeps_newest_first() {
        push_history("dashdot-test", vec![10.0]);
        push_history("dashdot-test", vec![30.0, 50.0]);
        let history = read_history("dashdot-test");
        assert_eq!(history[0], vec![30.0, 50.0]);
        assert_eq!(history[1], vec![10.0]);
        assert_eq!(average_of(&history[0]), 40.0);
    }

    #[test]
    fn parses_info_with_default_gpu_layout() {
        let info: Info = serde_json::from_value(json!({
            "os":{"distro":"Debian","kernel":"6.1","release":"12","uptime":5.0},
            "cpu":{"brand":"Intel","model":""},
            "ram":{"size":100.0},
            "storage":[{"size":10.0,"disks":[{"device":"sda","brand":"x","type":"SSD"}]}],
            "gpu":{}
        }))
        .unwrap();
        assert!(info.gpu.layout.is_empty());
        assert_eq!(info.storage[0].disks[0].device, "sda");
    }
}
