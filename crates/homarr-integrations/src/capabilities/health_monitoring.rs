use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCore {
    pub id: u32,
    pub utilization: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkThroughput {
    pub up: f64,
    pub down: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadAverage {
    #[serde(rename = "1min")]
    pub one_min: f64,
    #[serde(rename = "5min")]
    pub five_min: f64,
    #[serde(rename = "15min")]
    pub fifteen_min: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemInfo {
    pub device_name: String,
    pub used: String,
    pub available: String,
    /// Total capacity in bytes, when the provider reports it separately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    pub percentage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SmartStatusReason {
    AttributesWithinThresholds,
    AttributesFailed { attributes: Vec<String> },
    NoCriticalWarning,
    CriticalWarning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartInfo {
    pub device_name: String,
    pub temperature: Option<f64>,
    pub overall_status: String,
    pub healthy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<SmartStatusReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub gpu_id: String,
    pub name: String,
    pub memory_utilization: f64,
    pub processor_utilization: f64,
    pub temperature: Option<f64>,
    pub fan_speed: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemHealthMonitoring {
    pub version: String,
    pub cpu_model_name: String,
    pub cpu_utilization: f64,
    /// Usage per logical CPU, when the integration provides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<Vec<CpuCore>>,
    pub mem_used_in_bytes: f64,
    pub mem_available_in_bytes: f64,
    pub uptime: f64,
    pub network: Option<NetworkThroughput>,
    pub load_average: Option<LoadAverage>,
    pub reboot_required: bool,
    pub available_pkg_updates: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_temp: Option<f64>,
    pub file_system: Vec<FileSystemInfo>,
    pub smart: Vec<SmartInfo>,
    pub gpu: Vec<GpuInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCpu {
    pub utilization: f64,
    pub cores: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMemory {
    pub used: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStorage {
    pub used: f64,
    pub total: f64,
    pub read: Option<f64>,
    pub write: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNetwork {
    #[serde(rename = "in")]
    pub in_: Option<f64>,
    pub out: Option<f64>,
}

/// Shared fields of node / lxc / qemu resources (TS `ComputeResourceBase`).
/// `resource_type` is `"node"`, `"lxc"` or `"qemu"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeResourceBase {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    pub node: String,
    pub is_running: bool,
    pub status: String,
    pub id: String,
    pub cpu: ResourceCpu,
    pub memory: ResourceMemory,
    pub storage: ResourceStorage,
    pub network: ResourceNetwork,
    /// Seconds.
    pub uptime: f64,
    /// HA service status (for HA managed VMs).
    pub ha_state: Option<String>,
}

pub type NodeResource = ComputeResourceBase;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LxcResource {
    #[serde(flatten)]
    pub base: ComputeResourceBase,
    pub vm_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QemuResource {
    #[serde(flatten)]
    pub base: ComputeResourceBase,
    pub vm_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageResource {
    /// Always `"storage"`.
    #[serde(rename = "type")]
    pub resource_type: String,
    pub name: String,
    pub node: String,
    pub is_running: bool,
    pub status: String,
    pub id: String,
    pub storage_plugin: String,
    pub used: f64,
    pub total: f64,
    pub is_shared: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterHealthMonitoring {
    pub nodes: Vec<NodeResource>,
    pub lxcs: Vec<LxcResource>,
    pub vms: Vec<QemuResource>,
    pub storages: Vec<StorageResource>,
}
