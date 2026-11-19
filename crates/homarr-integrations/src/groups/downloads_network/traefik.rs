//! Traefik (port of `traefik/*`): dashboard data from the read-only API.

use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::Value;

use super::http;
use crate::capabilities::traefik::{
    TraefikDashboardData, TraefikProtocol, TraefikProtocolSummary, TraefikResourceDetail,
    TraefikResourceStatus, TraefikResourceSummary, TraefikResourceType, TraefikUdpSummary,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const MAX_RESOURCE_DETAILS: usize = 200;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Default, Deserialize)]
struct Resource {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Version {
    #[serde(rename = "Version", default)]
    version: Option<String>,
}

fn normalize_status(status: Option<&str>) -> TraefikResourceStatus {
    match status.map(str::to_lowercase).as_deref() {
        Some("enabled") => TraefikResourceStatus::Enabled,
        Some("disabled") => TraefikResourceStatus::Disabled,
        Some("warning") => TraefikResourceStatus::Warning,
        Some("error") => TraefikResourceStatus::Error,
        _ => TraefikResourceStatus::Unknown,
    }
}

fn summarize(resources: &[Resource]) -> TraefikResourceSummary {
    let mut summary = TraefikResourceSummary::default();
    for resource in resources {
        summary.total += 1;
        match normalize_status(resource.status.as_deref()) {
            TraefikResourceStatus::Enabled => summary.enabled += 1,
            TraefikResourceStatus::Warning => summary.warnings += 1,
            TraefikResourceStatus::Error => summary.errors += 1,
            _ => {}
        }
    }
    summary
}

fn protocol_summary(
    routers: &[Resource],
    services: &[Resource],
    middlewares: &[Resource],
) -> TraefikProtocolSummary {
    TraefikProtocolSummary {
        routers: summarize(routers),
        services: summarize(services),
        middlewares: summarize(middlewares),
    }
}

fn details(
    protocol: TraefikProtocol,
    resource_type: TraefikResourceType,
    resources: &[Resource],
    remaining: usize,
) -> Vec<TraefikResourceDetail> {
    let type_name = match resource_type {
        TraefikResourceType::Router => "router",
        TraefikResourceType::Service => "service",
        TraefikResourceType::Middleware => "middleware",
    };
    resources
        .iter()
        .take(remaining)
        .enumerate()
        .map(|(index, resource)| TraefikResourceDetail {
            protocol,
            resource_type,
            name: resource
                .name
                .clone()
                .unwrap_or_else(|| format!("{type_name}-{}", index + 1)),
            provider: resource.provider.clone(),
            status: normalize_status(resource.status.as_deref()),
        })
        .collect()
}

/// Everything the dashboard needs; a failed endpoint is reported in `failed_endpoints` and contributes nothing.
struct Fetched {
    version: Option<Version>,
    entry_points: Option<Vec<Resource>>,
    http_routers: Option<Vec<Resource>>,
    http_services: Option<Vec<Resource>>,
    http_middlewares: Option<Vec<Resource>>,
    tcp_routers: Option<Vec<Resource>>,
    tcp_services: Option<Vec<Resource>>,
    tcp_middlewares: Option<Vec<Resource>>,
    udp_routers: Option<Vec<Resource>>,
    udp_services: Option<Vec<Resource>>,
}

fn build_dashboard(fetched: Fetched) -> TraefikDashboardData {
    let failed = |name: &str, failed: bool| failed.then(|| name.to_string());
    let failed_endpoints: Vec<String> = [
        failed("version", fetched.version.is_none()),
        failed("entrypoints", fetched.entry_points.is_none()),
        failed("http/routers", fetched.http_routers.is_none()),
        failed("http/services", fetched.http_services.is_none()),
        failed("http/middlewares", fetched.http_middlewares.is_none()),
        failed("tcp/routers", fetched.tcp_routers.is_none()),
        failed("tcp/services", fetched.tcp_services.is_none()),
        failed("tcp/middlewares", fetched.tcp_middlewares.is_none()),
        failed("udp/routers", fetched.udp_routers.is_none()),
        failed("udp/services", fetched.udp_services.is_none()),
    ]
    .into_iter()
    .flatten()
    .collect();

    let version = fetched.version.and_then(|version| version.version);
    let entry_points = fetched.entry_points.unwrap_or_default();
    let http_routers = fetched.http_routers.unwrap_or_default();
    let http_services = fetched.http_services.unwrap_or_default();
    let http_middlewares = fetched.http_middlewares.unwrap_or_default();
    let tcp_routers = fetched.tcp_routers.unwrap_or_default();
    let tcp_services = fetched.tcp_services.unwrap_or_default();
    let tcp_middlewares = fetched.tcp_middlewares.unwrap_or_default();
    let udp_routers = fetched.udp_routers.unwrap_or_default();
    let udp_services = fetched.udp_services.unwrap_or_default();

    use TraefikProtocol::{Http, Tcp, Udp};
    use TraefikResourceType::{Middleware, Router, Service};
    let groups: [(TraefikProtocol, TraefikResourceType, &[Resource]); 8] = [
        (Http, Router, &http_routers),
        (Http, Service, &http_services),
        (Http, Middleware, &http_middlewares),
        (Tcp, Router, &tcp_routers),
        (Tcp, Service, &tcp_services),
        (Tcp, Middleware, &tcp_middlewares),
        (Udp, Router, &udp_routers),
        (Udp, Service, &udp_services),
    ];
    let mut resources: Vec<TraefikResourceDetail> = Vec::new();
    for (protocol, resource_type, entries) in groups {
        let remaining = MAX_RESOURCE_DETAILS - resources.len();
        if remaining == 0 {
            break;
        }
        resources.extend(details(protocol, resource_type, entries, remaining));
    }

    TraefikDashboardData {
        version,
        entry_points: entry_points
            .iter()
            .take(MAX_RESOURCE_DETAILS)
            .enumerate()
            .map(|(index, entry)| {
                entry
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("entrypoint-{}", index + 1))
            })
            .collect(),
        resources,
        failed_endpoints,
        http: protocol_summary(&http_routers, &http_services, &http_middlewares),
        tcp: protocol_summary(&tcp_routers, &tcp_services, &tcp_middlewares),
        udp: TraefikUdpSummary {
            routers: summarize(&udp_routers),
            services: summarize(&udp_services),
        },
    }
}

fn authorize(req: reqwest::RequestBuilder, ctx: &IntegrationContext) -> reqwest::RequestBuilder {
    if let (Some(user), Some(pass)) = (ctx.secret("username"), ctx.secret("password")) {
        return req.header(
            reqwest::header::AUTHORIZATION,
            http::basic_auth_value(user, pass),
        );
    }
    if let Some(key) = ctx.secret("apiKey") {
        return req.bearer_auth(key);
    }
    req
}

/// Reads at most `MAX_RESPONSE_BYTES` of JSON.
async fn get_bounded_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Value> {
    let url = http::url(ctx, path, &[])?;
    let req = authorize(http::get(rt, url), ctx).timeout(REQUEST_TIMEOUT);
    let mut resp = http::send(ctx, req).await?;
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|err| crate::error::request_error(ctx, &err))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(HomarrError::Upstream(format!(
                "Traefik response from {} is larger than {} bytes",
                ctx.name, MAX_RESPONSE_BYTES
            )));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|err| crate::error::parse_error(ctx, "the Traefik response", err))
}

async fn resources(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<Vec<Resource>> {
    let value = get_bounded_json(rt, ctx, path).await?;
    http::from_value(ctx, "the Traefik resources", value)
}

async fn version(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<Version> {
    let value = get_bounded_json(rt, ctx, "/api/version").await?;
    http::from_value(ctx, "the Traefik version", value)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    version(rt, ctx).await.map(drop)
}

pub async fn dashboard(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<TraefikDashboardData> {
    let (version_result, entry_points, hr, hs, hm, tr, ts, tm, ur, us) = tokio::join!(
        version(rt, ctx),
        resources(rt, ctx, "/api/entrypoints"),
        resources(rt, ctx, "/api/http/routers"),
        resources(rt, ctx, "/api/http/services"),
        resources(rt, ctx, "/api/http/middlewares"),
        resources(rt, ctx, "/api/tcp/routers"),
        resources(rt, ctx, "/api/tcp/services"),
        resources(rt, ctx, "/api/tcp/middlewares"),
        resources(rt, ctx, "/api/udp/routers"),
        resources(rt, ctx, "/api/udp/services"),
    );
    let all_failed = version_result.is_err()
        && [&entry_points, &hr, &hs, &hm, &tr, &ts, &tm, &ur, &us]
            .iter()
            .all(|result| result.is_err());
    if all_failed {
        // Every endpoint failed: surface the version error, it is the most telling one.
        return Err(version_result
            .err()
            .unwrap_or_else(|| HomarrError::Upstream("Every Traefik endpoint failed".into())));
    }
    Ok(build_dashboard(Fetched {
        version: version_result.ok(),
        entry_points: entry_points.ok(),
        http_routers: hr.ok(),
        http_services: hs.ok(),
        http_middlewares: hm.ok(),
        tcp_routers: tr.ok(),
        tcp_services: ts.ok(),
        tcp_middlewares: tm.ok(),
        udp_routers: ur.ok(),
        udp_services: us.ok(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resources(value: Value) -> Option<Vec<Resource>> {
        Some(serde_json::from_value(value).unwrap())
    }

    fn empty() -> Fetched {
        Fetched {
            version: Some(Version {
                version: Some("3.1.0".into()),
            }),
            entry_points: resources(json!([])),
            http_routers: resources(json!([])),
            http_services: resources(json!([])),
            http_middlewares: resources(json!([])),
            tcp_routers: resources(json!([])),
            tcp_services: resources(json!([])),
            tcp_middlewares: resources(json!([])),
            udp_routers: resources(json!([])),
            udp_services: resources(json!([])),
        }
    }

    #[test]
    fn summarizes_statuses_case_insensitively() {
        let list: Vec<Resource> = serde_json::from_value(json!([
            {"name": "a", "status": "enabled"}, {"name": "b", "status": "WARNING"},
            {"status": "error"}, {"name": "d", "status": "disabled"}, {"name": "e"}
        ]))
        .unwrap();
        let summary = summarize(&list);
        assert_eq!(
            summary,
            TraefikResourceSummary {
                total: 5,
                enabled: 1,
                warnings: 1,
                errors: 1
            }
        );
    }

    #[test]
    fn builds_dashboard_and_reports_failed_endpoints() {
        let mut fetched = empty();
        fetched.http_routers = resources(json!([
            {"name": "web@docker", "provider": "docker", "status": "enabled", "rule": "Host(`x`)"},
            {"status": "disabled"}
        ]));
        fetched.entry_points = resources(json!([{"name": "web"}, {}]));
        fetched.tcp_services = None;
        let out = build_dashboard(fetched);
        assert_eq!(out.version.as_deref(), Some("3.1.0"));
        assert_eq!(out.entry_points, vec!["web", "entrypoint-2"]);
        assert_eq!(out.failed_endpoints, vec!["tcp/services"]);
        assert_eq!(out.http.routers.total, 2);
        assert_eq!(out.resources.len(), 2);
        assert_eq!(out.resources[0].provider.as_deref(), Some("docker"));
        assert_eq!(out.resources[1].name, "router-2");
        assert_eq!(out.resources[1].status, TraefikResourceStatus::Disabled);
    }

    #[test]
    fn caps_resource_details() {
        let mut fetched = empty();
        let many: Vec<Value> = (0..250).map(|i| json!({"name": format!("r{i}")})).collect();
        fetched.http_routers = resources(Value::Array(many.clone()));
        fetched.http_services = resources(Value::Array(many));
        let out = build_dashboard(fetched);
        assert_eq!(out.resources.len(), MAX_RESOURCE_DETAILS);
        assert_eq!(out.http.services.total, 250);
    }
}
