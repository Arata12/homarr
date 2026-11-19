//! OpenMediaVault (`openmediavault/*`): JSON-RPC (`/rpc.php`) with header or cookie sessions and optional TOTP.

use chrono::Utc;
use hmac::{Hmac, Mac};
use homarr_common::{HomarrError, HomarrResult};
use reqwest::{Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;

use super::http::{self, as_f64, cookie_header, str_of};
use crate::capabilities::health_monitoring::{
    FileSystemInfo, LoadAverage, SmartInfo, SystemHealthMonitoring,
};
use crate::error::{parse_error, request_error, status_error};
use crate::http_auth::ResolvedHttpAuth;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Session {
    #[serde(rename_all = "camelCase")]
    Header { session_id: String },
    #[serde(rename_all = "camelCase")]
    Cookie {
        login_token: String,
        session_id: String,
    },
}

impl Session {
    fn cookie(&self) -> Option<String> {
        match self {
            Session::Cookie {
                login_token,
                session_id,
            } => Some(format!("{login_token};{session_id}")),
            Session::Header { .. } => None,
        }
    }

    fn apply(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Session::Header { session_id } => req.header("X-OPENMEDIAVAULT-SESSIONID", session_id),
            Session::Cookie { .. } => req.header("Cookie", self.cookie().unwrap_or_default()),
        }
    }
}

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

const INVALID_SECRET: &str =
    "Use the Base32 shared secret from OpenMediaVault two-factor enrollment, not a 6-digit code.";

fn decode_base32(secret: &str) -> Result<Vec<u8>, &'static str> {
    let encoded: String = secret
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .trim_end_matches('=')
        .to_uppercase();
    if encoded.len() < 16 || !encoded.bytes().all(|b| BASE32_ALPHABET.contains(&b)) {
        return Err(INVALID_SECRET);
    }
    let (mut bits, mut value) = (0u32, 0u32);
    let mut bytes = Vec::new();
    for byte in encoded.bytes() {
        let index = BASE32_ALPHABET.iter().position(|b| *b == byte).unwrap_or(0) as u32;
        value = (value << 5) | index;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push(((value >> bits) & 0xff) as u8);
            value &= (1 << bits) - 1;
        }
    }
    if value != 0 || ![0, 2, 4, 5, 7].contains(&(encoded.len() % 8)) {
        return Err(INVALID_SECRET);
    }
    Ok(bytes)
}

/// RFC 6238 TOTP (SHA-1, 30 second step, 6 digits).
pub fn generate_totp_code(secret: &str, timestamp_ms: i64) -> Result<String, &'static str> {
    let key = decode_base32(secret)?;
    let counter = (timestamp_ms / 30_000) as u64;
    let mut mac = Hmac::<Sha1>::new_from_slice(&key).map_err(|_| INVALID_SECRET)?;
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let code = u32::from_be_bytes([
        digest[offset],
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]) & 0x7fff_ffff;
    Ok(format!("{:06}", code % 1_000_000))
}

fn totp_code(ctx: &IntegrationContext) -> HomarrResult<String> {
    let secret = ctx.secret("totpSecret").ok_or_else(|| {
        HomarrError::BadRequest(format!(
            "{}: Add the Base32 TOTP secret, or use a separate OpenMediaVault admin account without TOTP.",
            ctx.name
        ))
    })?;
    generate_totp_code(secret, Utc::now().timestamp_millis())
        .map_err(|message| HomarrError::BadRequest(format!("{}: {message}", ctx.name)))
}

fn totp_rejected(ctx: &IntegrationContext) -> HomarrError {
    HomarrError::Upstream(format!(
        "{}: OpenMediaVault rejected the TOTP code. Check the secret and sync both servers' clocks.",
        ctx.name
    ))
}

async fn rpc_call(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    service: &str,
    method: &str,
    params: Value,
    session: Option<&Session>,
    extra_cookie: Option<&str>,
) -> HomarrResult<Response> {
    let mut req = http::request(rt, ctx, Method::POST, "/rpc.php", &[])?
        .header("User-Agent", "Homarr")
        .json(&json!({ "service": service, "method": method, "params": params }));
    if let Some(session) = session {
        req = session.apply(req);
    }
    if let Some(cookie) = extra_cookie {
        req = req.header("Cookie", cookie);
    }
    req.send().await.map_err(|err| request_error(ctx, &err))
}

fn auth_response(value: &Value) -> Value {
    match value.get("response") {
        Some(response) if response.is_object() => response.clone(),
        _ => json!({}),
    }
}

fn find_cookie(response: &Response, names: [&str; 2]) -> Option<String> {
    response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .find(|pair| names.iter().any(|n| pair.contains(n)))
        .map(str::to_string)
}

async fn new_session(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Session> {
    if ctx.secret("totpSecret").is_some() {
        totp_code(ctx)?;
    }
    let response = rpc_call(
        rt,
        ctx,
        "session",
        "login",
        json!({ "username": ctx.require_secret("username")?, "password": ctx.require_secret("password")? }),
        None,
        None,
    )
    .await?;
    if !response.status().is_success() {
        return Err(status_error(ctx, response.status()));
    }
    let set_cookies = cookie_header(&response);
    let session_cookie = find_cookie(
        &response,
        ["X-OPENMEDIAVAULT-SESSIONID", "OPENMEDIAVAULT-SESSIONID"],
    );
    let login_cookie = find_cookie(
        &response,
        ["X-OPENMEDIAVAULT-LOGIN", "OPENMEDIAVAULT-LOGIN"],
    );
    let body: Value = response
        .json()
        .await
        .map_err(|err| parse_error(ctx, "login response", err))?;
    let data = auth_response(&body);

    let challenge = data.get("challenge").filter(|c| c.is_object());
    if str_of(&data, "status") == Some("challengeRequired") || challenge.is_some() {
        if challenge
            .and_then(|c| c.get("kind"))
            .is_some_and(|kind| kind.as_str() != Some("totp"))
        {
            return Err(HomarrError::Upstream(format!(
                "{}: Homarr only supports OpenMediaVault TOTP challenges.",
                ctx.name
            )));
        }
        let code = totp_code(ctx)?;
        let cookie = set_cookies.ok_or_else(|| totp_rejected(ctx))?;
        let verified = rpc_call(
            rt,
            ctx,
            "session",
            "verify",
            json!({ "code": code }),
            None,
            Some(&cookie),
        )
        .await?;
        if !verified.status().is_success() {
            return Err(totp_rejected(ctx));
        }
        let verified_body: Value = verified.json().await.map_err(|_| totp_rejected(ctx))?;
        return match str_of(&auth_response(&verified_body), "sessionid").filter(|s| !s.is_empty()) {
            Some(session_id) => Ok(Session::Header {
                session_id: session_id.to_string(),
            }),
            None => Err(totp_rejected(ctx)),
        };
    }

    if let Some(session_id) = str_of(&data, "sessionid").filter(|s| !s.is_empty()) {
        return Ok(Session::Header {
            session_id: session_id.to_string(),
        });
    }
    match (login_cookie, session_cookie) {
        (Some(login_token), Some(session_id)) => Ok(Session::Cookie {
            login_token,
            session_id,
        }),
        _ => Err(status_error(ctx, StatusCode::UNAUTHORIZED)),
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    new_session(rt, ctx).await.map(|_| ())
}

/// Credentials for custom widget HTTP requests (port of `getHttpAuthenticationAsync`): a fresh session.
pub async fn http_auth(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ResolvedHttpAuth> {
    Ok(session_http_auth(&new_session(rt, ctx).await?))
}

fn session_http_auth(session: &Session) -> ResolvedHttpAuth {
    let (header, value, redact_values) = match session {
        Session::Header { session_id } => (
            "X-OPENMEDIAVAULT-SESSIONID",
            session_id.clone(),
            vec![session_id.clone()],
        ),
        Session::Cookie {
            login_token,
            session_id,
        } => {
            let cookie = format!("{login_token};{session_id}");
            (
                "Cookie",
                cookie.clone(),
                vec![cookie, login_token.clone(), session_id.clone()],
            )
        }
    };
    ResolvedHttpAuth {
        headers: [(header.to_string(), value)].into(),
        redact_values,
        ..Default::default()
    }
}

/// Authenticated RPC call; a `401` with the stored session logs in again.
async fn authed_rpc(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    service: &str,
    method: &str,
    params: Value,
) -> HomarrResult<Value> {
    let mut response = None;
    if let Some(stored) = rt.sessions.get::<Session>(ctx) {
        let attempt = rpc_call(
            rt,
            ctx,
            service,
            method,
            params.clone(),
            Some(&stored),
            None,
        )
        .await?;
        if attempt.status() != StatusCode::UNAUTHORIZED {
            response = Some(attempt);
        } else {
            tracing::debug!(integration = %ctx.name, "Session expired, getting new session");
        }
    }
    let response = match response {
        Some(response) => response,
        None => {
            let fresh = new_session(rt, ctx).await?;
            rt.sessions.set(ctx, &fresh, None);
            let response = rpc_call(rt, ctx, service, method, params, Some(&fresh), None).await?;
            response
        }
    };
    if !response.status().is_success() {
        return Err(status_error(ctx, response.status()));
    }
    response
        .json()
        .await
        .map_err(|err| parse_error(ctx, &format!("{service}.{method}"), err))
}

fn string_number(value: &Value) -> Option<f64> {
    as_f64(value)
}

pub fn map_file_systems(
    ctx: &IntegrationContext,
    body: &Value,
) -> HomarrResult<Vec<FileSystemInfo>> {
    let invalid = || parse_error(ctx, "file systems", "Invalid file system response");
    body.get("response")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|fs| {
            Ok(FileSystemInfo {
                device_name: str_of(fs, "devicename").ok_or_else(invalid)?.to_string(),
                used: str_of(fs, "used").ok_or_else(invalid)?.to_string(),
                available: match fs.get("available").ok_or_else(invalid)? {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => return Err(invalid()),
                },
                size: None,
                percentage: fs
                    .get("percentage")
                    .and_then(Value::as_f64)
                    .ok_or_else(invalid)?,
            })
        })
        .collect()
}

pub fn map_smart(ctx: &IntegrationContext, body: &Value) -> HomarrResult<Vec<SmartInfo>> {
    let invalid = || {
        parse_error(
            ctx,
            "SMART information",
            "Invalid SMART information response",
        )
    };
    body.get("response")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|device| {
            let status = str_of(device, "overallstatus").ok_or_else(invalid)?;
            let temperature = match device.get("temperature").ok_or_else(invalid)? {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse::<f64>().ok().filter(|n| !n.is_nan()),
                _ => return Err(invalid()),
            };
            Ok(SmartInfo {
                device_name: str_of(device, "devicename")
                    .ok_or_else(invalid)?
                    .to_string(),
                temperature,
                // See openmediavault/system/storage/smartinformation.inc
                healthy: status == "GOOD",
                overall_status: status.to_string(),
                status_reason: None,
            })
        })
        .collect()
}

pub fn map_system_info(
    ctx: &IntegrationContext,
    system: &Value,
    file_system: Vec<FileSystemInfo>,
    smart: Vec<SmartInfo>,
    cpu_temp: Option<f64>,
) -> HomarrResult<SystemHealthMonitoring> {
    let invalid = |what: &str| {
        parse_error(
            ctx,
            "system information",
            format!("Invalid system information response ({what})"),
        )
    };
    let response = system
        .get("response")
        .filter(|r| r.is_object())
        .ok_or_else(|| invalid("response"))?;
    let load = response
        .get("loadAverage")
        .ok_or_else(|| invalid("loadAverage"))?;
    let load_value = |key: &str| {
        load.get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid(key))
    };
    // `memUsed` / `memAvailable` are strings in the RPC payload.
    let mem = |key: &str| {
        response
            .get(key)
            .filter(|v| v.is_string())
            .and_then(string_number)
            .ok_or_else(|| invalid(key))
    };
    Ok(SystemHealthMonitoring {
        version: str_of(response, "version")
            .ok_or_else(|| invalid("version"))?
            .to_string(),
        cpu_model_name: str_of(response, "cpuModelName")
            .unwrap_or("Unknown CPU")
            .to_string(),
        cpu_utilization: response
            .get("cpuUtilization")
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid("cpuUtilization"))?,
        cpu_cores: None,
        mem_used_in_bytes: mem("memUsed")?,
        mem_available_in_bytes: mem("memAvailable")?,
        uptime: response
            .get("uptime")
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid("uptime"))?,
        // Real-time traffic monitoring is not available over the RPC API.
        network: None,
        load_average: Some(LoadAverage {
            one_min: load_value("1min")?,
            five_min: load_value("5min")?,
            fifteen_min: load_value("15min")?,
        }),
        reboot_required: response
            .get("rebootRequired")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid("rebootRequired"))?,
        available_pkg_updates: response
            .get("availablePkgUpdates")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("availablePkgUpdates"))? as u32,
        cpu_temp,
        file_system,
        smart,
        gpu: Vec::new(), // OpenMediaVault does not expose GPU monitoring via its RPC API.
    })
}

pub async fn file_systems(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<FileSystemInfo>> {
    let body = authed_rpc(
        rt,
        ctx,
        "filesystemmgmt",
        "enumerateMountedFilesystems",
        json!({ "includeroot": true }),
    )
    .await?;
    map_file_systems(ctx, &body)
}

pub async fn system_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SystemHealthMonitoring> {
    let system = authed_rpc(rt, ctx, "system", "getInformation", json!({})).await?;
    let file_system = file_systems(rt, ctx).await?;
    let smart = authed_rpc(rt, ctx, "smart", "enumerateDevices", json!({})).await?;
    let cpu_temp = authed_rpc(rt, ctx, "cputemp", "get", json!({})).await?;
    let smart = map_smart(ctx, &smart)?;
    let cpu_temp = cpu_temp
        .pointer("/response/cputemp")
        .and_then(Value::as_f64);
    map_system_info(ctx, &system, file_system, smart, cpu_temp)
}

/// `(value, label)` entries for the storage widget's volume picker.
pub async fn list_storage_volumes(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<(String, String)>> {
    Ok(file_systems(rt, ctx)
        .await?
        .into_iter()
        .map(|fs| {
            (
                format!("{}:{}", ctx.id, fs.device_name),
                format!("{} ({})", fs.device_name, ctx.name),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "omv".into(),
            name: "OMV".into(),
            url: "http://omv".into(),
            external_url: None,
            kind: IntegrationKind::Openmediavault,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn totp_matches_rfc6238_vectors() {
        // RFC 6238 secret "12345678901234567890" in Base32, SHA-1, last 6 digits of the 8 digit vectors.
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        assert_eq!(generate_totp_code(secret, 59_000).unwrap(), "287082");
        assert_eq!(
            generate_totp_code(secret, 1_111_111_109_000).unwrap(),
            "081804"
        );
        assert_eq!(
            generate_totp_code(secret, 1_234_567_890_000).unwrap(),
            "005924"
        );
    }

    #[test]
    fn rejects_invalid_totp_secrets() {
        assert!(generate_totp_code("123456", 0).is_err());
        assert!(generate_totp_code("not base32 !!!!!!!!", 0).is_err());
        assert!(generate_totp_code("GEZD-GNBV GY3T-QOJQ GEZD-GNBV GY3T-QOJQ", 0).is_ok());
    }

    #[test]
    fn builds_http_auth_for_header_and_cookie_sessions() {
        let header = session_http_auth(&Session::Header {
            session_id: "sid".into(),
        });
        assert_eq!(header.headers["X-OPENMEDIAVAULT-SESSIONID"], "sid");
        assert_eq!(header.redact_values, ["sid"]);
        let cookie = session_http_auth(&Session::Cookie {
            login_token: "a=1".into(),
            session_id: "b=2".into(),
        });
        assert_eq!(cookie.headers["Cookie"], "a=1;b=2");
        assert_eq!(cookie.redact_values, ["a=1;b=2", "a=1", "b=2"]);
    }

    #[test]
    fn session_serialization_roundtrips_and_builds_cookie() {
        let session = Session::Cookie {
            login_token: "a=1".into(),
            session_id: "b=2".into(),
        };
        assert_eq!(session.cookie().as_deref(), Some("a=1;b=2"));
        let json = serde_json::to_value(&session).unwrap();
        assert_eq!(serde_json::from_value::<Session>(json).unwrap(), session);
        assert_eq!(
            Session::Header {
                session_id: "x".into()
            }
            .cookie(),
            None
        );
    }

    #[test]
    fn maps_system_information() {
        let system = json!({"response": {
            "version": "7.4", "cpuModelName": null, "cpuUtilization": 12.5, "memUsed": "100", "memAvailable": "900",
            "uptime": 1000, "loadAverage": {"1min": 1.0, "5min": 2.0, "15min": 3.0}, "rebootRequired": true, "availablePkgUpdates": 3
        }});
        let info = map_system_info(&ctx(), &system, vec![], vec![], Some(55.0)).unwrap();
        assert_eq!(info.cpu_model_name, "Unknown CPU");
        assert_eq!(info.mem_used_in_bytes, 100.0);
        assert_eq!(info.available_pkg_updates, 3);
        assert!(info.reboot_required);
        assert_eq!(info.cpu_temp, Some(55.0));
        assert!(map_system_info(&ctx(), &json!({"response": {}}), vec![], vec![], None).is_err());
    }

    #[test]
    fn maps_file_systems_and_smart() {
        let fs = map_file_systems(
            &ctx(),
            &json!({"response":[{"devicename":"sda1","used":"10","available":90,"percentage":10}]}),
        )
        .unwrap();
        assert_eq!(fs[0].available, "90");
        assert!(map_file_systems(&ctx(), &json!({"response":[{"devicename":"sda1"}]})).is_err());
        let smart = map_smart(
            &ctx(),
            &json!({"response":[
                {"devicename":"sda","temperature":"34","overallstatus":"GOOD"},
                {"devicename":"sdb","temperature":"n/a","overallstatus":"BAD"}
            ]}),
        )
        .unwrap();
        assert_eq!(smart[0].temperature, Some(34.0));
        assert!(smart[0].healthy);
        assert_eq!(smart[1].temperature, None);
        assert!(!smart[1].healthy);
    }
}
