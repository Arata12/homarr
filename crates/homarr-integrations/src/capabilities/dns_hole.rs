use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DnsHoleStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsHoleSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DnsHoleStatus>,
    pub domains_being_blocked: u64,
    pub ads_blocked_today: u64,
    pub ads_blocked_today_percentage: f64,
    pub dns_queries_today: u64,
}
