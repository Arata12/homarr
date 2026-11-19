//! Proxmox VE (`proxmox/proxmox-integration.ts`): cluster health through the API token.

use homarr_common::HomarrResult;
use serde_json::Value;

use super::http::{self, f64_of, str_of};
use crate::capabilities::health_monitoring::{
    ClusterHealthMonitoring, ComputeResourceBase, LxcResource, QemuResource, ResourceCpu,
    ResourceMemory, ResourceNetwork, ResourceStorage, StorageResource,
};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

fn auth_header(ctx: &IntegrationContext) -> HomarrResult<String> {
    let token_id = format!(
        "{}@{}!{}",
        ctx.require_secret("username")?,
        ctx.require_secret("realm")?,
        ctx.require_secret("tokenId")?
    );
    Ok(format!(
        "PVEAPIToken={token_id}={}",
        ctx.require_secret("apiKey")?
    ))
}

async fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    what: &str,
) -> HomarrResult<Value> {
    let req = http::get(rt, ctx, path, &[])?.header("Authorization", auth_header(ctx)?);
    http::send_json(ctx, req, what).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    get(rt, ctx, "/api2/json/nodes", "nodes").await.map(|_| ())
}

pub async fn cluster_health(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ClusterHealthMonitoring> {
    let response = get(rt, ctx, "/api2/json/cluster/resources", "cluster resources").await?;
    let resources = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| parse_error(ctx, "cluster resources", "missing data array"))?;
    tracing::info!(
        total = resources.len(),
        "Found resources in Proxmox cluster"
    );
    Ok(map_resources(resources))
}

fn compute_base(resource_type: &str, resource: &Value) -> ComputeResourceBase {
    let status = str_of(resource, "status")
        .map(str::to_string)
        .unwrap_or_else(|| {
            if resource_type == "node" {
                "offline".into()
            } else {
                "stopped".into()
            }
        });
    ComputeResourceBase {
        resource_type: resource_type.to_string(),
        name: str_of(resource, "name").unwrap_or_default().to_string(),
        node: str_of(resource, "node").unwrap_or_default().to_string(),
        is_running: matches!(str_of(resource, "status"), Some("running" | "online")),
        status,
        id: str_of(resource, "id").unwrap_or_default().to_string(),
        cpu: ResourceCpu {
            utilization: f64_of(resource, "cpu").unwrap_or(0.0),
            cores: f64_of(resource, "maxcpu").unwrap_or(0.0),
        },
        memory: ResourceMemory {
            used: f64_of(resource, "mem").unwrap_or(0.0),
            total: f64_of(resource, "maxmem").unwrap_or(0.0),
        },
        storage: ResourceStorage {
            used: f64_of(resource, "disk").unwrap_or(0.0),
            total: f64_of(resource, "maxdisk").unwrap_or(0.0),
            read: f64_of(resource, "diskread"),
            write: f64_of(resource, "diskwrite"),
        },
        network: ResourceNetwork {
            in_: f64_of(resource, "netin"),
            out: f64_of(resource, "netout"),
        },
        uptime: f64_of(resource, "uptime").unwrap_or(0.0),
        ha_state: str_of(resource, "hastate").map(str::to_string),
    }
}

pub fn map_resources(resources: &[Value]) -> ClusterHealthMonitoring {
    let mut result = ClusterHealthMonitoring {
        nodes: Vec::new(),
        lxcs: Vec::new(),
        vms: Vec::new(),
        storages: Vec::new(),
    };
    for resource in resources {
        let vm_id = f64_of(resource, "vmid").unwrap_or(0.0) as u64;
        match str_of(resource, "type") {
            Some("node") => {
                let mut node = compute_base("node", resource);
                node.name = str_of(resource, "node").unwrap_or_default().to_string();
                result.nodes.push(node);
            }
            Some("lxc") => result.lxcs.push(LxcResource {
                base: compute_base("lxc", resource),
                vm_id,
            }),
            Some("qemu") => result.vms.push(QemuResource {
                base: compute_base("qemu", resource),
                vm_id,
            }),
            Some("storage") => {
                let status = str_of(resource, "status");
                result.storages.push(StorageResource {
                    resource_type: "storage".into(),
                    name: str_of(resource, "storage").unwrap_or_default().to_string(),
                    node: str_of(resource, "node").unwrap_or_default().to_string(),
                    is_running: status == Some("available"),
                    status: status.unwrap_or("offline").to_string(),
                    id: str_of(resource, "id").unwrap_or_default().to_string(),
                    storage_plugin: str_of(resource, "storage").unwrap_or_default().to_string(),
                    used: f64_of(resource, "disk").unwrap_or(0.0),
                    total: f64_of(resource, "maxdisk").unwrap_or(0.0),
                    is_shared: f64_of(resource, "shared") == Some(1.0),
                });
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_cluster_resources() {
        let resources = vec![
            json!({"id":"node/pve","type":"node","node":"pve","status":"online","cpu":0.25,"maxcpu":8,"mem":4,"maxmem":8,"disk":1,"maxdisk":2,"uptime":100}),
            json!({"id":"lxc/100","type":"lxc","vmid":100,"name":"ct","node":"pve","status":"running","netin":5,"hastate":"started"}),
            json!({"id":"qemu/101","type":"qemu","vmid":101,"name":"vm","node":"pve"}),
            json!({"id":"storage/pve/local","type":"storage","storage":"local","node":"pve","status":"available","disk":3,"maxdisk":9,"shared":1}),
            json!({"id":"sdn/x","type":"sdn"}),
        ];
        let result = map_resources(&resources);
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].name, "pve");
        assert!(result.nodes[0].is_running);
        assert_eq!(result.nodes[0].cpu.cores, 8.0);
        assert_eq!(result.lxcs[0].vm_id, 100);
        assert_eq!(result.lxcs[0].base.network.in_, Some(5.0));
        assert_eq!(result.lxcs[0].base.ha_state.as_deref(), Some("started"));
        assert_eq!(result.vms[0].base.status, "stopped");
        assert!(!result.vms[0].base.is_running);
        assert_eq!(result.storages[0].name, "local");
        assert!(result.storages[0].is_shared && result.storages[0].is_running);
    }
}
