use serde::{Deserialize, Serialize};

/// Normalized UPS status, resolved from the raw NUT status flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpsStatus {
    Online,
    Charging,
    OnBattery,
    LowBattery,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsSummary {
    /// Stable identifier of the UPS device on the NUT server.
    pub id: String,
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub status: UpsStatus,
    /// Percent.
    pub battery_charge: Option<f64>,
    /// Seconds.
    pub battery_runtime: Option<f64>,
    pub battery_voltage: Option<f64>,
    /// Percent.
    pub load: Option<f64>,
    pub input_voltage: Option<f64>,
    pub output_voltage: Option<f64>,
    /// Watts.
    pub power: Option<f64>,
    /// Degrees celsius.
    pub temperature: Option<f64>,
}
