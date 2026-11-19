//! Traefik dashboard data (`traefik/traefik-types.ts`; Traefik has no interface file in TS, the widget called
//! the integration class directly).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraefikResourceStatus {
    Enabled,
    Disabled,
    Warning,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraefikResourceSummary {
    pub total: u64,
    pub enabled: u64,
    pub warnings: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraefikProtocolSummary {
    pub routers: TraefikResourceSummary,
    pub services: TraefikResourceSummary,
    pub middlewares: TraefikResourceSummary,
}

/// UDP has no middlewares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraefikUdpSummary {
    pub routers: TraefikResourceSummary,
    pub services: TraefikResourceSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraefikProtocol {
    Http,
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraefikResourceType {
    Router,
    Service,
    Middleware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraefikResourceDetail {
    pub protocol: TraefikProtocol,
    #[serde(rename = "type")]
    pub resource_type: TraefikResourceType,
    pub name: String,
    pub provider: Option<String>,
    pub status: TraefikResourceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraefikDashboardData {
    pub version: Option<String>,
    pub entry_points: Vec<String>,
    pub resources: Vec<TraefikResourceDetail>,
    pub failed_endpoints: Vec<String>,
    pub http: TraefikProtocolSummary,
    pub tcp: TraefikProtocolSummary,
    pub udp: TraefikUdpSummary,
}
