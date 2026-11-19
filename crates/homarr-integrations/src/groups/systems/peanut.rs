//! PeaNUT (`peanut/peanut-integration.ts`): UPS summaries from the NUT web UI API.

use homarr_common::HomarrResult;
use serde_json::{Map, Value};

use super::http::{self, basic_auth};
use crate::capabilities::ups::{UpsStatus, UpsSummary};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

fn auth_header(ctx: &IntegrationContext) -> Option<String> {
    Some(basic_auth(ctx.secret("username")?, ctx.secret("password")?))
}

fn devices_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    query: &[(&str, &str)],
) -> HomarrResult<reqwest::RequestBuilder> {
    let mut req = http::get(rt, ctx, "/api/v1/devices", query)?;
    if let Some(header) = auth_header(ctx) {
        req = req.header("Authorization", header);
    }
    Ok(req)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    http::send_json(ctx, devices_request(rt, ctx, &[])?, "devices")
        .await
        .map(|_| ())
}

pub async fn ups_summaries(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<UpsSummary>> {
    let value = http::send_json(
        ctx,
        devices_request(rt, ctx, &[("meta", "true")])?,
        "devices",
    )
    .await?;
    map_devices(ctx, &value)
}

pub fn map_devices(ctx: &IntegrationContext, value: &Value) -> HomarrResult<Vec<UpsSummary>> {
    let devices = value
        .as_array()
        .ok_or_else(|| parse_error(ctx, "PeaNUT devices", "expected an array"))?;
    devices
        .iter()
        .enumerate()
        .map(|(index, device)| {
            let device = device
                .as_object()
                .filter(|o| o.values().all(|v| v.is_string() || v.is_number()))
                .ok_or_else(|| parse_error(ctx, "PeaNUT devices", "unexpected device shape"))?;
            Ok(map_device(device, index))
        })
        .collect()
}

fn read_string(device: &Map<String, Value>, key: &str) -> Option<String> {
    let value = match device.get(key)? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!value.is_empty()).then_some(value)
}

fn read_number(device: &Map<String, Value>, key: &str) -> Option<f64> {
    match device.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) if !s.is_empty() => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
        _ => None,
    }
}

fn map_device(device: &Map<String, Value>, index: usize) -> UpsSummary {
    let manufacturer = read_string(device, "device.mfr").or_else(|| read_string(device, "ups.mfr"));
    let model = read_string(device, "device.model").or_else(|| read_string(device, "ups.model"));
    let id = read_string(device, "peanut.device_id").unwrap_or_else(|| format!("ups-{index}"));
    let name = [manufacturer.as_deref(), model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    UpsSummary {
        name: if name.is_empty() { id.clone() } else { name },
        id,
        manufacturer,
        model,
        serial: read_string(device, "device.serial"),
        status: parse_status(read_string(device, "ups.status").as_deref()),
        battery_charge: read_number(device, "battery.charge"),
        battery_runtime: read_number(device, "battery.runtime"),
        battery_voltage: read_number(device, "battery.voltage"),
        load: read_number(device, "ups.load"),
        input_voltage: read_number(device, "input.voltage"),
        output_voltage: read_number(device, "output.voltage"),
        power: read_number(device, "ups.realpower").or_else(|| read_number(device, "ups.power")),
        temperature: read_number(device, "ups.temperature"),
    }
}

/// Resolves the raw NUT `ups.status` flags (e.g. "OL CHRG", "OB DISCHRG LB") into one status,
/// checking the most critical flags first.
pub fn parse_status(raw: Option<&str>) -> UpsStatus {
    let upper = raw.unwrap_or_default().to_uppercase();
    let flags: Vec<&str> = upper.split_whitespace().collect();
    if flags.contains(&"LB") {
        UpsStatus::LowBattery
    } else if flags.contains(&"OB") {
        UpsStatus::OnBattery
    } else if flags.contains(&"OL") {
        if flags.contains(&"CHRG") {
            UpsStatus::Charging
        } else {
            UpsStatus::Online
        }
    } else {
        UpsStatus::Unknown
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "p".into(),
            name: "PeaNUT".into(),
            url: "http://peanut".into(),
            external_url: None,
            kind: IntegrationKind::PeaNut,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn status_priority() {
        assert_eq!(parse_status(Some("OB DISCHRG LB")), UpsStatus::LowBattery);
        assert_eq!(parse_status(Some("OB")), UpsStatus::OnBattery);
        assert_eq!(parse_status(Some("ol chrg")), UpsStatus::Charging);
        assert_eq!(parse_status(Some("OL")), UpsStatus::Online);
        assert_eq!(parse_status(None), UpsStatus::Unknown);
    }

    #[test]
    fn maps_device() {
        let value = json!([
            {"device.mfr":"APC","device.model":"Back-UPS","device.serial":"S1","ups.status":"OL","battery.charge":"100","battery.runtime":1200,"ups.load":"12","ups.realpower":"60","peanut.device_id":"apc"},
            {"ups.status":"OB"}
        ]);
        let ups = map_devices(&ctx(), &value).unwrap();
        assert_eq!(ups[0].id, "apc");
        assert_eq!(ups[0].name, "APC Back-UPS");
        assert_eq!(ups[0].battery_charge, Some(100.0));
        assert_eq!(ups[0].power, Some(60.0));
        assert_eq!(ups[1].id, "ups-1");
        assert_eq!(ups[1].name, "ups-1");
        assert_eq!(ups[1].status, UpsStatus::OnBattery);
        assert!(map_devices(&ctx(), &json!({})).is_err());
    }
}
