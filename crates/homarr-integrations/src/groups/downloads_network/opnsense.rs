//! OPNsense (port of `opnsense/*`). API docs: https://docs.opnsense.org/development/api.html#core-api

use std::time::Duration;

use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::Value;

use super::http;
use crate::capabilities::firewall::{
    FirewallCpuSummary, FirewallInterface, FirewallInterfacesSummary, FirewallMemorySummary,
    FirewallVersionSummary,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const SYSTEM_INFORMATION: &str = "/api/diagnostics/system/system_information";
const SYSTEM_RESOURCES: &str = "/api/diagnostics/system/system_resources";
const TRAFFIC_INTERFACE: &str = "/api/diagnostics/traffic/interface";
const CPU_STREAM: &str = "/api/diagnostics/cpu_usage/stream";

/// Chunks read from the CPU event stream before giving up (the first chunk is skipped, like in TS).
const MAX_STREAM_CHUNKS: usize = 10;
const STREAM_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct SystemSummary {
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryResponse {
    memory: MemoryInfo,
}

#[derive(Debug, Deserialize)]
struct MemoryInfo {
    total: String,
    used: f64,
}

#[derive(Debug, Deserialize)]
struct InterfacesResponse {
    interfaces: std::collections::BTreeMap<String, InterfaceInfo>,
}

#[derive(Debug, Deserialize)]
struct InterfaceInfo {
    #[serde(rename = "bytes received")]
    bytes_received: String,
    #[serde(rename = "bytes transmitted")]
    bytes_transmitted: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct CpuValues {
    total: f64,
}

fn map_version(summary: &SystemSummary) -> FirewallVersionSummary {
    FirewallVersionSummary {
        version: summary
            .versions
            .first()
            .cloned()
            .unwrap_or_else(|| "Unknown".into()),
    }
}

fn map_memory(memory: &MemoryInfo) -> HomarrResult<FirewallMemorySummary> {
    let total: f64 = memory.total.trim().parse().map_err(|_| {
        HomarrError::Upstream(format!(
            "OPNsense returned an invalid total memory '{}'",
            memory.total
        ))
    })?;
    Ok(FirewallMemorySummary {
        total,
        used: memory.used,
        percent: 100.0 * memory.used / total,
    })
}

fn parse_bytes(text: &str) -> HomarrResult<f64> {
    text.trim().parse().map_err(|_| {
        HomarrError::Upstream(format!(
            "OPNsense returned an invalid byte counter '{text}'"
        ))
    })
}

fn map_interfaces(response: &InterfacesResponse) -> HomarrResult<Vec<FirewallInterface>> {
    response
        .interfaces
        .values()
        .map(|interface| {
            Ok(FirewallInterface {
                name: interface.name.clone(),
                receive: parse_bytes(&interface.bytes_received)?,
                transmit: parse_bytes(&interface.bytes_transmitted)?,
            })
        })
        .collect()
}

/// First `data:` line of a chunk parsed as CPU values.
fn parse_cpu_chunk(chunk: &str) -> HomarrResult<Option<CpuValues>> {
    for line in chunk.split('\n') {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let values = serde_json::from_str(data.trim()).map_err(|err| {
            HomarrError::Upstream(format!("Failed to parse OPNsense CPU data: {err}"))
        })?;
        return Ok(Some(values));
    }
    Ok(None)
}

async fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<reqwest::Response> {
    let key = ctx.require_secret("opnsenseApiKey")?;
    let secret = ctx.require_secret("opnsenseApiSecret")?;
    let url = http::url(ctx, path, &[])?;
    let req = http::get(rt, url).header(
        reqwest::header::AUTHORIZATION,
        http::basic_auth_value(key, secret),
    );
    http::send(ctx, req).await
}

async fn get_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Value> {
    http::read_json(ctx, get(rt, ctx, path).await?).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    if get_json(rt, ctx, SYSTEM_INFORMATION).await?.is_object() {
        Ok(())
    } else {
        Err(HomarrError::Upstream(format!(
            "Failed to parse the system information from {}: expected object data",
            ctx.name
        )))
    }
}

pub async fn version(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<FirewallVersionSummary> {
    let value = get_json(rt, ctx, SYSTEM_INFORMATION).await?;
    let summary: SystemSummary = http::from_value(ctx, "the OPNsense system information", value)?;
    Ok(map_version(&summary))
}

pub async fn memory(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<FirewallMemorySummary> {
    let value = get_json(rt, ctx, SYSTEM_RESOURCES).await?;
    let response: MemoryResponse = http::from_value(ctx, "the OPNsense memory usage", value)?;
    map_memory(&response.memory)
}

/// A snapshot of the interface byte counters (the TS version kept a 15 entry history in Redis and returned the
/// newest entry; consumers derive rates from consecutive snapshots).
pub async fn interfaces(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<FirewallInterfacesSummary>> {
    let value = get_json(rt, ctx, TRAFFIC_INTERFACE).await?;
    let response: InterfacesResponse =
        http::from_value(ctx, "the OPNsense interface traffic", value)?;
    Ok(vec![FirewallInterfacesSummary {
        data: map_interfaces(&response)?,
        timestamp: Utc::now(),
    }])
}

pub async fn cpu(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<FirewallCpuSummary> {
    let mut resp = get(rt, ctx, CPU_STREAM).await?;
    let read = async {
        // The first chunk is skipped: it only contains the initial (zero) sample.
        for chunk_index in 1..=MAX_STREAM_CHUNKS {
            let Some(chunk) = resp
                .chunk()
                .await
                .map_err(|err| crate::error::request_error(ctx, &err))?
            else {
                break;
            };
            if chunk_index < 2 {
                continue;
            }
            if let Some(values) = parse_cpu_chunk(&String::from_utf8_lossy(&chunk))? {
                return Ok(FirewallCpuSummary {
                    total: values.total,
                });
            }
        }
        Err(HomarrError::Upstream(format!(
            "{} returned no valid CPU data",
            ctx.name
        )))
    };
    tokio::time::timeout(STREAM_TIMEOUT, read)
        .await
        .map_err(|_| HomarrError::Upstream(format!("Request to {} timed out", ctx.name)))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_version_with_fallback() {
        let summary = SystemSummary {
            versions: vec!["OPNsense 24.7".into(), "other".into()],
        };
        assert_eq!(map_version(&summary).version, "OPNsense 24.7");
        assert_eq!(
            map_version(&SystemSummary { versions: vec![] }).version,
            "Unknown"
        );
    }

    #[test]
    fn maps_memory() {
        let memory: MemoryResponse =
            serde_json::from_value(json!({"memory": {"total": "1000", "used": 250}})).unwrap();
        let out = map_memory(&memory.memory).unwrap();
        assert_eq!(out.total, 1000.0);
        assert_eq!(out.percent, 25.0);
        let bad = MemoryInfo {
            total: "x".into(),
            used: 1.0,
        };
        assert!(map_memory(&bad).is_err());
    }

    #[test]
    fn maps_interfaces() {
        let response: InterfacesResponse = serde_json::from_value(json!({
            "interfaces": {
                "wan": {"bytes received": "100", "bytes transmitted": "200", "name": "WAN"},
                "lan": {"bytes received": "1", "bytes transmitted": "2", "name": "LAN"}
            },
            "time": 1
        }))
        .unwrap();
        let out = map_interfaces(&response).unwrap();
        assert_eq!(out.len(), 2);
        let wan = out.iter().find(|i| i.name == "WAN").unwrap();
        assert_eq!((wan.receive, wan.transmit), (100.0, 200.0));
    }

    #[test]
    fn parses_cpu_stream_chunks() {
        assert!(parse_cpu_chunk("retry: 1000\n\n").unwrap().is_none());
        let values = parse_cpu_chunk("event: x\ndata: {\"total\": 12, \"user\": 3}\n\n")
            .unwrap()
            .unwrap();
        assert_eq!(values.total, 12.0);
        assert!(parse_cpu_chunk("data: nope\n").is_err());
    }
}
