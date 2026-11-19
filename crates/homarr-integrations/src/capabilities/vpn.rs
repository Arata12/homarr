use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProviderDetails {
    pub protocol: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnSummary {
    pub vpn_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_status: Option<String>,
    pub public_ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vpn_provider: Option<VpnProviderDetails>,
}
