//! TrueNAS (`truenas/*`): middleware API over a WebSocket (JSON-RPC 2.0 on 24.10+, legacy DDP before).
//!
//! A connection is opened per call (connect, authenticate, query, close) instead of being shared across calls
//! as the TypeScript client does.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};

use super::http::{self, f64_of, str_of};
use crate::capabilities::health_monitoring::{
    FileSystemInfo, NetworkThroughput, SmartInfo, SystemHealthMonitoring,
};
use crate::error::{parse_error, status_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Bytes per second conversion for Netdata's kilobit per second values.
const NETWORK_MULTIPLIER: f64 = 1000.0 / 8.0;
const MEBIBYTE: f64 = 1024.0 * 1024.0;
/// Allow Netdata to finish its collection interval before requesting the latest sample.
const REPORTING_DELAY_SECONDS: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ApiKind {
    JsonRpc,
    Legacy,
}

impl ApiKind {
    fn path(self) -> &'static str {
        match self {
            ApiKind::JsonRpc => "/api/current",
            ApiKind::Legacy => "/websocket",
        }
    }

    fn requires_session_handshake(self) -> bool {
        self == ApiKind::Legacy
    }

    fn build_request(self, id: &str, method: &str, params: &[Value]) -> Value {
        match self {
            ApiKind::JsonRpc => {
                json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
            }
            ApiKind::Legacy => {
                json!({ "id": id, "msg": "method", "method": method, "params": params })
            }
        }
    }

    /// `Some(Ok(result))` / `Some(Err(error))` when `message` answers `id`, otherwise `None`.
    fn match_response(self, message: &Value, id: &str) -> Option<Result<Value, Value>> {
        if str_of(message, "id") != Some(id) {
            return None;
        }
        match self {
            ApiKind::JsonRpc if str_of(message, "jsonrpc") != Some("2.0") => return None,
            ApiKind::Legacy if str_of(message, "msg") != Some("result") => return None,
            _ => {}
        }
        match message.get("error").filter(|e| !e.is_null()) {
            Some(error) => Some(Err(error.clone())),
            None => Some(Ok(message.get("result").cloned().unwrap_or(Value::Null))),
        }
    }
}

type WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Socket {
    ws: WebSocket,
    api: ApiKind,
    next_id: u64,
}

/// Why opening a socket failed: protocol failures (e.g. a missing JSON-RPC endpoint) allow the legacy fallback.
enum OpenError {
    Protocol(HomarrError),
    Network(HomarrError),
}

fn websocket_url(ctx: &IntegrationContext, path: &str) -> HomarrResult<url::Url> {
    let mut url = http::url(ctx, path, &[])?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|()| HomarrError::BadRequest(format!("Invalid TrueNAS URL for {}", ctx.name)))?;
    Ok(url)
}

fn network_error(ctx: &IntegrationContext, err: &WsError) -> HomarrError {
    let text = err.to_string();
    let lower = text.to_lowercase();
    HomarrError::Upstream(
        if lower.contains("certificate") || lower.contains("unknownissuer") {
            format!("Request to {} failed: the TLS certificate is not trusted. Add the certificate to the trusted certificates or use http", ctx.name)
        } else {
            format!(
                "Unable to connect to {} at {}: {text}",
                ctx.name,
                ctx.base_url()
            )
        },
    )
}

async fn open_socket(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    api: ApiKind,
) -> Result<Socket, OpenError> {
    let url = websocket_url(ctx, api.path()).map_err(OpenError::Network)?;
    let connector = if url.scheme() == "wss" {
        Some(Connector::Rustls(
            rt.ws_tls_config().map_err(OpenError::Network)?,
        ))
    } else {
        None
    };
    let connect = connect_async_tls_with_config(url.as_str(), None, false, connector);
    let (ws, _) = match tokio::time::timeout(REQUEST_TIMEOUT, connect).await {
        Err(_) => {
            return Err(OpenError::Network(HomarrError::Upstream(format!(
                "Request to {} timed out",
                ctx.name
            ))))
        }
        Ok(Err(err @ WsError::Http(_))) | Ok(Err(err @ WsError::HttpFormat(_))) => {
            return Err(OpenError::Protocol(HomarrError::Upstream(format!(
                "{} rejected the WebSocket connection: {err}",
                ctx.name
            ))))
        }
        Ok(Err(err)) => return Err(OpenError::Network(network_error(ctx, &err))),
        Ok(Ok(connection)) => connection,
    };
    let mut socket = Socket {
        ws,
        api,
        next_id: 0,
    };
    if api.requires_session_handshake() {
        socket
            .register_session(ctx)
            .await
            .map_err(OpenError::Network)?;
    }
    Ok(socket)
}

impl Socket {
    async fn read_json(&mut self, ctx: &IntegrationContext) -> HomarrResult<Option<Value>> {
        loop {
            match self.ws.next().await {
                None | Some(Ok(Message::Close(_))) => {
                    return Err(HomarrError::Upstream(format!(
                        "Connection to {} closed before a response was received",
                        ctx.name
                    )))
                }
                Some(Err(err)) => return Err(network_error(ctx, &err)),
                Some(Ok(Message::Text(text))) => {
                    return Ok(serde_json::from_str(text.as_str()).ok())
                }
                Some(Ok(_)) => continue,
            }
        }
    }

    /// The legacy DDP API requires a `connect` handshake before any method can be called.
    async fn register_session(&mut self, ctx: &IntegrationContext) -> HomarrResult<()> {
        let hello = json!({ "msg": "connect", "version": "1", "support": ["1"] });
        self.ws
            .send(Message::Text(hello.to_string().into()))
            .await
            .map_err(|err| network_error(ctx, &err))?;
        let wait = async {
            loop {
                match self.read_json(ctx).await? {
                    Some(message) if str_of(&message, "msg") == Some("connected") => return Ok(()),
                    Some(message) if str_of(&message, "msg") == Some("failed") => {
                        return Err(HomarrError::Upstream(format!(
                            "Unable to establish TrueNAS session on {}",
                            ctx.name
                        )))
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(REQUEST_TIMEOUT, wait)
            .await
            .unwrap_or_else(|_| {
                Err(HomarrError::Upstream(format!(
                    "Request to {} timed out",
                    ctx.name
                )))
            })
    }

    async fn request(
        &mut self,
        ctx: &IntegrationContext,
        method: &str,
        params: Vec<Value>,
    ) -> HomarrResult<Value> {
        self.next_id += 1;
        let id = format!("homarr-{}", self.next_id);
        let payload = self.api.build_request(&id, method, &params);
        self.ws
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|err| network_error(ctx, &err))?;
        let api = self.api;
        let wait = async {
            loop {
                let Some(message) = self.read_json(ctx).await? else {
                    continue;
                };
                if let Some(response) = api.match_response(&message, &id) {
                    return response.map_err(|error| {
                        HomarrError::Upstream(format!(
                            "TrueNAS method \"{method}\" on {} failed: {}",
                            ctx.name,
                            truncate(&error.to_string())
                        ))
                    });
                }
            }
        };
        tokio::time::timeout(REQUEST_TIMEOUT, wait)
            .await
            .unwrap_or_else(|_| {
                Err(HomarrError::Upstream(format!(
                    "Request to {} timed out",
                    ctx.name
                )))
            })
    }

    /// Authenticates with the API key when one is configured, otherwise with username and password.
    async fn authenticate(&mut self, ctx: &IntegrationContext) -> HomarrResult<()> {
        let result = match ctx.secret("apiKey") {
            Some(key) => {
                self.request(ctx, "auth.login_with_api_key", vec![json!(key)])
                    .await?
            }
            None => {
                self.request(
                    ctx,
                    "auth.login",
                    vec![
                        json!(ctx.require_secret("username")?),
                        json!(ctx.require_secret("password")?),
                    ],
                )
                .await?
            }
        };
        match result.as_bool() {
            Some(true) => Ok(()),
            Some(false) => Err(status_error(ctx, reqwest::StatusCode::UNAUTHORIZED)),
            None => Err(parse_error(ctx, "login response", "expected a boolean")),
        }
    }

    async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

fn truncate(text: &str) -> String {
    const LIMIT: usize = 200;
    if text.chars().count() > LIMIT {
        format!("{}…", text.chars().take(LIMIT).collect::<String>())
    } else {
        text.to_string()
    }
}

/// Connects using the JSON-RPC API (TrueNAS 24.10+) when available and falls back to the legacy API otherwise.
async fn connect(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Socket> {
    if let Some(api) = rt.sessions.get::<ApiKind>(ctx) {
        return open_socket(rt, ctx, api).await.map_err(|e| match e {
            OpenError::Protocol(err) | OpenError::Network(err) => err,
        });
    }
    let socket = match open_socket(rt, ctx, ApiKind::JsonRpc).await {
        Ok(socket) => socket,
        Err(OpenError::Network(err)) => return Err(err),
        Err(OpenError::Protocol(err)) => {
            tracing::debug!(integration = %ctx.name, %err, "JSON-RPC API unavailable, falling back to the legacy websocket API");
            match open_socket(rt, ctx, ApiKind::Legacy).await {
                Ok(socket) => socket,
                Err(OpenError::Protocol(err) | OpenError::Network(err)) => return Err(err),
            }
        }
    };
    rt.sessions.set(ctx, &socket.api, None);
    Ok(socket)
}

async fn connect_authenticated(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Socket> {
    let mut socket = connect(rt, ctx).await?;
    if let Err(err) = socket.authenticate(ctx).await {
        socket.close().await;
        return Err(err);
    }
    Ok(socket)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    // The test may run with updated credentials: forget the detected API.
    rt.sessions.clear(ctx);
    connect_authenticated(rt, ctx).await?.close().await;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pool {
    pub name: String,
    pub status: String,
    pub healthy: bool,
    pub used: f64,
    pub available: f64,
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let mut socket = connect_authenticated(rt, ctx).await?;
    let result = collect(&mut socket, ctx).await;
    socket.close().await;
    let (system_info, reporting, pools, netdata) = result?;
    build_system_health(ctx, &system_info, &reporting, pools, &netdata)
}

async fn collect(
    socket: &mut Socket,
    ctx: &IntegrationContext,
) -> HomarrResult<(Value, Value, Vec<Pool>, Value)> {
    let system_info = socket.request(ctx, "system.info", vec![]).await?;
    let now = Utc::now().timestamp();
    let start = now - 5 * 60;
    let end = now - REPORTING_DELAY_SECONDS;
    let reporting = socket
        .request(
            ctx,
            "reporting.get_data",
            vec![
                json!([{ "name": "cpu" }, { "name": "memory" }, { "name": "cputemp" }]),
                json!({ "aggregate": true, "start": start, "end": end }),
            ],
        )
        .await?;
    let pools = pools(socket, ctx).await?;
    let interfaces = socket
        .request(ctx, "interface.query", vec![json!([]), json!({})])
        .await?;
    let queries: Vec<Value> = interfaces
        .as_array()
        .ok_or_else(|| parse_error(ctx, "network interfaces", "expected an array"))?
        .iter()
        .filter_map(|i| str_of(i, "id"))
        .map(|id| json!({ "name": "interface", "identifier": id }))
        .collect();
    let netdata = socket
        .request(
            ctx,
            "reporting.netdata_get_data",
            vec![Value::Array(queries), json!({ "start": start, "end": end })],
        )
        .await?;
    Ok((system_info, reporting, pools, netdata))
}

async fn pools(socket: &mut Socket, ctx: &IntegrationContext) -> HomarrResult<Vec<Pool>> {
    let response = socket
        .request(
            ctx,
            "pool.query",
            vec![json!([]), json!({ "extra": { "is_upgraded": true } })],
        )
        .await?;
    let active = active_pools(ctx, &response)?;
    if active.is_empty() {
        return Ok(Vec::new());
    }
    let names: Vec<&str> = active.iter().map(|p| p.name.as_str()).collect();
    // Usable space accounts for RAIDZ parity and dataset limits; failures fall back to physical pool space.
    let usable = match socket
        .request(
            ctx,
            "pool.dataset.query",
            vec![
                json!([["id", "in", names]]),
                json!({ "extra": { "properties": ["used", "available"] } }),
            ],
        )
        .await
    {
        Ok(datasets) => usable_space_by_pool(&datasets),
        Err(error) => {
            tracing::warn!(integration = %ctx.name, %error, "Could not retrieve root dataset space, continuing with physical pool space");
            HashMap::new()
        }
    };
    Ok(active
        .into_iter()
        .map(|pool| match usable.get(&pool.name) {
            Some((used, available)) => Pool { used: *used, available: *available, ..pool },
            None => {
                tracing::warn!(integration = %ctx.name, pool = %pool.name, "Falling back to physical pool space, the root dataset reported no usable space");
                pool
            }
        })
        .collect())
}

/// Offline / exported pools report `null` for allocated, size and free and are left out.
pub fn active_pools(ctx: &IntegrationContext, response: &Value) -> HomarrResult<Vec<Pool>> {
    let invalid = || parse_error(ctx, "pools", "unexpected pool.query response");
    let pools = response.as_array().ok_or_else(invalid)?;
    let mut active = Vec::new();
    for pool in pools {
        let name = str_of(pool, "name").ok_or_else(invalid)?;
        let status = str_of(pool, "status").ok_or_else(invalid)?;
        let healthy = pool
            .get("healthy")
            .and_then(Value::as_bool)
            .ok_or_else(invalid)?;
        let (allocated, size, free) = (
            f64_of(pool, "allocated"),
            f64_of(pool, "size"),
            f64_of(pool, "free"),
        );
        if let (Some(allocated), Some(_), Some(free)) = (allocated, size, free) {
            active.push(Pool {
                name: name.to_string(),
                status: status.to_string(),
                healthy,
                used: allocated,
                available: free,
            });
        }
    }
    Ok(active)
}

pub fn usable_space_by_pool(datasets: &Value) -> HashMap<String, (f64, f64)> {
    datasets
        .as_array()
        .map(|datasets| {
            datasets
                .iter()
                .filter_map(|dataset| {
                    let id = str_of(dataset, "id")?;
                    let used = dataset.pointer("/used/parsed")?.as_f64()?;
                    let available = dataset.pointer("/available/parsed")?.as_f64()?;
                    Some((id.to_string(), (used, available)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Latest sample of a reporting item as `column -> value` (the `time` column is skipped).
pub fn extract_latest_reporting(
    ctx: &IntegrationContext,
    data: &Value,
    identifier: &str,
) -> HomarrResult<HashMap<String, f64>> {
    let item = data.as_array().and_then(|items| {
        items
            .iter()
            .find(|i| str_of(i, "identifier") == Some(identifier))
    });
    let latest = item
        .and_then(|i| i.get("data"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.last())
        .and_then(Value::as_array)
        .filter(|row| !row.is_empty());
    let (Some(item), Some(latest)) = (item, latest) else {
        return Err(parse_error(
            ctx,
            "reporting",
            format!("TrueNAS returned no \"{identifier}\" reporting data"),
        ));
    };
    let legend: Vec<&str> = item
        .get("legend")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut unique = legend.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != legend.len() {
        return Err(parse_error(
            ctx,
            "reporting",
            format!("TrueNAS returned duplicate \"{identifier}\" reporting columns"),
        ));
    }
    let mut values = HashMap::new();
    for (index, column) in legend.iter().enumerate() {
        if *column == "time" {
            continue;
        }
        let value = latest.get(index).and_then(Value::as_f64).ok_or_else(|| {
            parse_error(
                ctx,
                "reporting",
                format!("TrueNAS returned no \"{identifier}.{column}\" reporting value"),
            )
        })?;
        values.insert((*column).to_string(), value);
    }
    if values.is_empty() {
        return Err(parse_error(
            ctx,
            "reporting",
            format!("TrueNAS returned no \"{identifier}\" reporting values"),
        ));
    }
    Ok(values)
}

/// Sum of the latest `column` ("sent" / "received") over all interfaces.
pub fn extract_network_traffic(netdata: &Value, column: &str) -> f64 {
    netdata
        .as_array()
        .map(|reports| {
            reports
                .iter()
                .map(|report| {
                    let index = report
                        .get("legend")
                        .and_then(Value::as_array)
                        .and_then(|l| l.iter().position(|c| c.as_str() == Some(column)));
                    index
                        .and_then(|i| {
                            report
                                .get("data")
                                .and_then(Value::as_array)
                                .and_then(|rows| rows.last())
                                .and_then(|row| row.get(i))
                        })
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0)
                })
                .sum()
        })
        .unwrap_or(0.0)
}

pub fn build_system_health(
    ctx: &IntegrationContext,
    system_info: &Value,
    reporting: &Value,
    pools: Vec<Pool>,
    netdata: &Value,
) -> HomarrResult<SystemHealthMonitoring> {
    let invalid = |what: &str| parse_error(ctx, "system.info", format!("missing {what}"));
    let physmem = f64_of(system_info, "physmem")
        .filter(|m| *m > 0.0)
        .ok_or_else(|| invalid("physmem"))?;
    let cpu_data = extract_latest_reporting(ctx, reporting, "cpu")?;
    let cpu_temp_data = extract_latest_reporting(ctx, reporting, "cputemp")?;
    let memory_data = extract_latest_reporting(ctx, reporting, "memory")?;
    if memory_data.values().any(|v| *v < 0.0) {
        return Err(parse_error(
            ctx,
            "reporting",
            "TrueNAS returned negative memory reporting values",
        ));
    }
    let memory_used = if let Some(available) = memory_data.get("available") {
        physmem - available
    } else if let (Some(used), true) = (
        memory_data.get("used"),
        ["free", "cached", "buffers"]
            .iter()
            .all(|c| memory_data.contains_key(*c)),
    ) {
        used * MEBIBYTE
    } else {
        return Err(parse_error(
            ctx,
            "reporting",
            "TrueNAS returned an unsupported memory reporting format",
        ));
    };
    let mem_used = memory_used.max(0.0).min(physmem);
    let cpu_utilization = cpu_data.get("cpu").copied().unwrap_or_else(|| {
        cpu_data
            .iter()
            .filter(|(column, _)| *column != "idle" && *column != "iowait")
            .map(|(_, v)| *v)
            .sum()
    });
    Ok(SystemHealthMonitoring {
        version: str_of(system_info, "version")
            .ok_or_else(|| invalid("version"))?
            .to_string(),
        cpu_model_name: str_of(system_info, "model")
            .ok_or_else(|| invalid("model"))?
            .to_string(),
        cpu_utilization: cpu_utilization.clamp(0.0, 100.0),
        cpu_cores: None,
        mem_used_in_bytes: mem_used,
        mem_available_in_bytes: physmem - mem_used,
        uptime: f64_of(system_info, "uptime_seconds").ok_or_else(|| invalid("uptime_seconds"))?,
        network: Some(NetworkThroughput {
            up: extract_network_traffic(netdata, "sent") * NETWORK_MULTIPLIER,
            down: extract_network_traffic(netdata, "received") * NETWORK_MULTIPLIER,
        }),
        load_average: None,
        reboot_required: false,
        available_pkg_updates: 0,
        cpu_temp: cpu_temp_data.values().copied().reduce(f64::max),
        file_system: pools
            .iter()
            .map(|pool| {
                let total = pool.used + pool.available;
                FileSystemInfo {
                    device_name: pool.name.clone(),
                    available: format!("{}", pool.available),
                    used: format!("{}", pool.used),
                    size: None,
                    percentage: if total > 0.0 {
                        pool.used / total * 100.0
                    } else {
                        0.0
                    },
                }
            })
            .collect(),
        smart: pools
            .iter()
            .map(|pool| SmartInfo {
                device_name: pool.name.clone(),
                temperature: None,
                overall_status: pool.status.clone(),
                healthy: pool.healthy,
                status_reason: None,
            })
            .collect(),
        gpu: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "t".into(),
            name: "TrueNAS".into(),
            url: "https://nas.local".into(),
            external_url: None,
            kind: IntegrationKind::Truenas,
            secrets: Map::new(),
        }
    }

    #[test]
    fn api_descriptors() {
        assert_eq!(ApiKind::JsonRpc.path(), "/api/current");
        assert!(!ApiKind::JsonRpc.requires_session_handshake());
        assert_eq!(ApiKind::Legacy.path(), "/websocket");
        assert!(ApiKind::Legacy.requires_session_handshake());
        assert_eq!(
            ApiKind::JsonRpc.build_request("id-1", "system.info", &[]),
            json!({"jsonrpc":"2.0","id":"id-1","method":"system.info","params":[]})
        );
        assert_eq!(
            ApiKind::Legacy.build_request("id-1", "system.info", &[]),
            json!({"id":"id-1","msg":"method","method":"system.info","params":[]})
        );
    }

    #[test]
    fn matches_responses() {
        let ok = json!({"jsonrpc":"2.0","id":"id-1","result":true});
        assert_eq!(
            ApiKind::JsonRpc.match_response(&ok, "id-1"),
            Some(Ok(json!(true)))
        );
        assert_eq!(ApiKind::JsonRpc.match_response(&ok, "id-2"), None);
        let err = json!({"jsonrpc":"2.0","id":"id-1","error":{"message":"Not authenticated"}});
        assert_eq!(
            ApiKind::JsonRpc.match_response(&err, "id-1"),
            Some(Err(json!({"message":"Not authenticated"})))
        );
        let legacy = json!({"msg":"result","id":"id-1","result":5});
        assert_eq!(
            ApiKind::Legacy.match_response(&legacy, "id-1"),
            Some(Ok(json!(5)))
        );
        assert_eq!(ApiKind::Legacy.match_response(&ok, "id-1"), None);
        assert_eq!(
            ApiKind::Legacy.match_response(&json!({"msg":"added","id":"id-1"}), "id-1"),
            None
        );
    }

    #[test]
    fn builds_websocket_urls() {
        assert_eq!(
            websocket_url(&ctx(), "/api/current").unwrap().as_str(),
            "wss://nas.local/api/current"
        );
        let mut plain = ctx();
        plain.url = "http://nas.local:8080/".into();
        assert_eq!(
            websocket_url(&plain, "/websocket").unwrap().as_str(),
            "ws://nas.local:8080/websocket"
        );
    }

    #[test]
    fn filters_offline_pools_and_prefers_usable_space() {
        let response = json!([
            {"name":"tank","status":"ONLINE","healthy":true,"free":600.0,"size":1000.0,"allocated":400.0},
            {"name":"cold","status":"OFFLINE","healthy":false,"free":null,"size":null,"allocated":null}
        ]);
        let pools = active_pools(&ctx(), &response).unwrap();
        assert_eq!(pools.len(), 1);
        let usable = usable_space_by_pool(&json!([
            {"id":"tank","used":{"parsed":300.0},"available":{"parsed":500.0}},
            {"id":"locked","used":null,"available":{"parsed":1.0}}
        ]));
        assert_eq!(usable["tank"], (300.0, 500.0));
        assert!(!usable.contains_key("locked"));
    }

    fn reporting() -> Value {
        json!([
            {"identifier":"cpu","legend":["time","cpu"],"data":[[1.0,10.0],[2.0,35.0]]},
            {"identifier":"memory","legend":["time","available"],"data":[[2.0,250.0]]},
            {"identifier":"cputemp","legend":["time","cpu0","cpu1"],"data":[[2.0,40.0,52.0]]}
        ])
    }

    #[test]
    fn builds_system_health() {
        let system_info = json!({"version":"TrueNAS-25.04","hostname":"nas","physmem":1000.0,"model":"Xeon","uptime_seconds":99.0});
        let netdata = json!([
            {"legend":["time","received","sent"],"data":[[1.0,8.0,16.0]]},
            {"legend":["time","received","sent"],"data":[[1.0,8.0,0.0]]}
        ]);
        let pools = vec![Pool {
            name: "tank".into(),
            status: "ONLINE".into(),
            healthy: true,
            used: 300.0,
            available: 700.0,
        }];
        let info =
            build_system_health(&ctx(), &system_info, &reporting(), pools, &netdata).unwrap();
        assert_eq!(info.cpu_utilization, 35.0);
        assert_eq!(info.cpu_temp, Some(52.0));
        assert_eq!(info.mem_used_in_bytes, 750.0);
        assert_eq!(info.mem_available_in_bytes, 250.0);
        let network = info.network.unwrap();
        assert_eq!(network.down, 16.0 * 125.0);
        assert_eq!(network.up, 16.0 * 125.0);
        assert_eq!(info.file_system[0].percentage, 30.0);
        assert!(info.smart[0].healthy);
    }

    #[test]
    fn rejects_bad_reporting() {
        let system_info = json!({"version":"v","physmem":1000.0,"model":"x","uptime_seconds":1.0});
        let negative = json!([
            {"identifier":"cpu","legend":["cpu"],"data":[[10.0]]},
            {"identifier":"memory","legend":["available"],"data":[[-1.0]]},
            {"identifier":"cputemp","legend":["cpu0"],"data":[[40.0]]}
        ]);
        assert!(build_system_health(&ctx(), &system_info, &negative, vec![], &json!([])).is_err());
        let duplicate = json!([{"identifier":"cpu","legend":["a","a"],"data":[[1.0,2.0]]}]);
        assert!(extract_latest_reporting(&ctx(), &duplicate, "cpu").is_err());
        assert!(extract_latest_reporting(&ctx(), &json!([]), "cpu").is_err());
    }

    #[test]
    fn sums_cpu_columns_without_idle() {
        let reporting = json!([
            {"identifier":"cpu","legend":["time","user","system","idle","iowait"],"data":[[1.0,10.0,5.0,80.0,5.0]]},
            {"identifier":"memory","legend":["used","free","cached","buffers"],"data":[[100.0,1.0,1.0,1.0]]},
            {"identifier":"cputemp","legend":["cpu0"],"data":[[40.0]]}
        ]);
        let system_info = json!({"version":"v","physmem":1.0e9,"model":"x","uptime_seconds":1.0});
        let info =
            build_system_health(&ctx(), &system_info, &reporting, vec![], &json!([])).unwrap();
        assert_eq!(info.cpu_utilization, 15.0);
        assert_eq!(info.mem_used_in_bytes, 100.0 * MEBIBYTE);
    }
}
