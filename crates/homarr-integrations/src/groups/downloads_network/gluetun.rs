//! Gluetun (port of `gluetun/*`).

use std::net::IpAddr;

use homarr_common::{HomarrError, HomarrResult};
use reqwest::RequestBuilder;
use serde::Deserialize;

use super::http;
use crate::capabilities::vpn::VpnSummary;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Deserialize)]
struct VpnStatus {
    status: String,
}

#[derive(Debug, Deserialize)]
struct PublicIp {
    public_ip: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    city: Option<String>,
}

const VPN_STATUSES: &[&str] = &["running", "stopped", "starting", "stopping", "crashed"];

fn validate(status: &VpnStatus, public_ip: &PublicIp) -> HomarrResult<()> {
    if !VPN_STATUSES.contains(&status.status.as_str()) {
        return Err(HomarrError::Upstream(format!(
            "Gluetun returned the unknown VPN status '{}'",
            status.status
        )));
    }
    if !public_ip.public_ip.is_empty() && public_ip.public_ip.parse::<IpAddr>().is_err() {
        return Err(HomarrError::Upstream(
            "Gluetun returned an invalid public IP".into(),
        ));
    }
    Ok(())
}

fn map_summary(status: VpnStatus, public_ip: PublicIp) -> VpnSummary {
    VpnSummary {
        vpn_status: status.status,
        dns_status: None,
        public_ip: public_ip.public_ip,
        country: public_ip.country,
        city: public_ip.city,
        vpn_provider: None,
    }
}

/// API key header, else basic auth, else anonymous.
fn authorize(req: RequestBuilder, ctx: &IntegrationContext) -> RequestBuilder {
    if let Some(key) = ctx.secret("apiKey") {
        return req.header("X-API-Key", key);
    }
    if let (Some(user), Some(pass)) = (ctx.secret("username"), ctx.secret("password")) {
        return req.header(
            reqwest::header::AUTHORIZATION,
            http::basic_auth_value(user, pass),
        );
    }
    req
}

async fn get_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<serde_json::Value> {
    let url = http::url(ctx, path, &[])?;
    http::send_json(ctx, authorize(http::get(rt, url), ctx)).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let url = http::url(ctx, "/v1/vpn/status", &[])?;
    http::send(ctx, authorize(http::get(rt, url), ctx))
        .await
        .map(drop)
}

pub async fn summary(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<VpnSummary> {
    let (status, public_ip) = tokio::join!(
        get_json(rt, ctx, "/v1/vpn/status"),
        get_json(rt, ctx, "/v1/publicip/ip"),
    );
    let status: VpnStatus = http::from_value(ctx, "the Gluetun VPN status", status?)?;
    let public_ip: PublicIp = http::from_value(ctx, "the Gluetun public IP", public_ip?)?;
    validate(&status, &public_ip)?;
    Ok(map_summary(status, public_ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(status: &str, ip: serde_json::Value) -> HomarrResult<VpnSummary> {
        let status: VpnStatus = serde_json::from_value(json!({ "status": status })).unwrap();
        let ip: PublicIp = serde_json::from_value(ip).unwrap();
        validate(&status, &ip)?;
        Ok(map_summary(status, ip))
    }

    #[test]
    fn maps_summary() {
        let out = parse(
            "running",
            json!({"public_ip": "1.2.3.4", "country": "NL", "city": "AMS", "region": "x"}),
        )
        .unwrap();
        assert_eq!(out.vpn_status, "running");
        assert_eq!(out.public_ip, "1.2.3.4");
        assert_eq!(out.country.as_deref(), Some("NL"));
        assert_eq!(out.city.as_deref(), Some("AMS"));
    }

    #[test]
    fn accepts_empty_and_ipv6_ip_but_rejects_garbage() {
        assert!(parse("stopped", json!({"public_ip": ""})).is_ok());
        assert!(parse("running", json!({"public_ip": "2001:db8::1"})).is_ok());
        assert!(parse("running", json!({"public_ip": "not-an-ip"})).is_err());
        assert!(parse("weird", json!({"public_ip": ""})).is_err());
    }
}
