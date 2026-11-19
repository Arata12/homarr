use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnabledStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WwwSummary {
    pub status: EnabledStatus,
    pub latency: f64,
    pub ping: f64,
    pub uptime: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsersSummary {
    pub status: EnabledStatus,
    pub users: u64,
    pub guests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnUsersSummary {
    pub status: EnabledStatus,
    pub users: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkControllerSummary {
    pub wan_status: EnabledStatus,
    pub www: WwwSummary,
    pub wifi: UsersSummary,
    pub lan: UsersSummary,
    pub vpn: VpnUsersSummary,
}
