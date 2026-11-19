use std::collections::HashMap;
use std::path::Path;

use bollard::container::{
    ListContainersOptions, LogsOptions, RemoveContainerOptions, RestartContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::service::ContainerSummary;
use bollard::{Docker, API_DEFAULT_VERSION};
use homarr_common::{HomarrError, HomarrResult};
use homarr_core::AppConfig;
use homarr_definitions::docker_discovery::{
    build_docker_service_url_candidates, match_docker_service, DockerServiceMatch,
    DockerServicePort, DockerServiceUrlCandidate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created: i64,
    pub endpoint: String,
    pub ports: Vec<DockerServicePort>,
    pub labels: HashMap<String, String>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredDockerService {
    pub container_id: String,
    pub container_name: String,
    pub image: String,
    pub state: String,
    pub service_match: DockerServiceMatch,
    pub url_candidates: Vec<DockerServiceUrlCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
struct EnvEndpointSpec {
    id: String,
    name: String,
    transport: EnvEndpointTransport,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum EnvEndpointTransport {
    Socket {
        path: String,
    },
    Tcp {
        host: String,
        port: u16,
        #[serde(rename = "allowInsecure", default)]
        allow_insecure: bool,
    },
    Tls {
        host: String,
        port: u16,
    },
}

#[derive(Clone)]
pub struct DockerService {
    config: AppConfig,
}

impl DockerService {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    fn configured_endpoints(&self) -> Vec<(String, String, Docker)> {
        let mut clients = Vec::new();

        if let Ok(raw) = std::env::var("DOCKER_ENDPOINTS") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                if let Ok(specs) = serde_json::from_str::<Vec<EnvEndpointSpec>>(trimmed) {
                    for spec in specs {
                        match spec.transport {
                            EnvEndpointTransport::Socket { path } => {
                                if Path::new(&path).exists() {
                                    if let Ok(client) =
                                        Docker::connect_with_socket(&path, 120, API_DEFAULT_VERSION)
                                    {
                                        clients.push((spec.id, spec.name, client));
                                    }
                                }
                            }
                            EnvEndpointTransport::Tcp {
                                host,
                                port,
                                allow_insecure,
                            } if allow_insecure => {
                                let addr = format!("http://{host}:{port}");
                                if let Ok(client) =
                                    Docker::connect_with_http(&addr, 120, API_DEFAULT_VERSION)
                                {
                                    clients.push((spec.id, host, client));
                                }
                            }
                            EnvEndpointTransport::Tls { host, port } => {
                                let addr = format!("http://{host}:{port}");
                                if let Ok(client) =
                                    Docker::connect_with_http(&addr, 120, API_DEFAULT_VERSION)
                                {
                                    clients.push((spec.id, host, client));
                                }
                            }
                            _ => {}
                        }
                    }
                    return clients;
                }
            }
        }

        for (idx, socket_path) in self.config.docker_socket_paths.iter().enumerate() {
            if Path::new(socket_path).exists() {
                if let Ok(client) =
                    Docker::connect_with_socket(socket_path, 120, API_DEFAULT_VERSION)
                {
                    clients.push((format!("socket-{idx}"), "socket".to_string(), client));
                }
            }
        }

        for (idx, host) in self.config.docker_hostnames.iter().enumerate() {
            let port = self.config.docker_ports.get(idx).copied().unwrap_or(2375);
            let addr = format!("http://{host}:{port}");
            if let Ok(client) = Docker::connect_with_http(&addr, 120, API_DEFAULT_VERSION) {
                clients.push((format!("tcp-{idx}"), host.clone(), client));
            }
        }

        clients
    }

    pub async fn list_containers(&self) -> HomarrResult<Vec<DockerContainerInfo>> {
        let clients = self.configured_endpoints();
        if clients.is_empty() {
            return Ok(Vec::new());
        }

        let mut all = Vec::new();
        for (_id, endpoint_host, client) in clients {
            let opts = Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            });
            match client.list_containers(opts).await {
                Ok(list) => {
                    for c in list {
                        all.push(map_container_summary(&endpoint_host, c));
                    }
                }
                Err(err) => {
                    tracing::warn!(endpoint = %endpoint_host, error = %err, "Failed to list Docker containers");
                }
            }
        }
        all.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(all)
    }

    pub async fn get_container(&self, id: &str) -> HomarrResult<DockerContainerInfo> {
        let all = self.list_containers().await?;
        all.into_iter()
            .find(|c| c.id == id || c.id.starts_with(id) || c.name == id)
            .ok_or_else(|| HomarrError::NotFound(format!("Container '{id}' not found")))
    }

    pub async fn container_action(&self, id: &str, action: &str) -> HomarrResult<()> {
        let (endpoint_host, client) = self.find_client_for_container(id).await?;
        match action {
            "start" => client
                .start_container(id, None::<StartContainerOptions<String>>)
                .await
                .map_err(|e| {
                    HomarrError::Upstream(format!("Docker start failed on {endpoint_host}: {e}"))
                })?,
            "stop" => client
                .stop_container(id, Some(StopContainerOptions { t: 10 }))
                .await
                .map_err(|e| {
                    HomarrError::Upstream(format!("Docker stop failed on {endpoint_host}: {e}"))
                })?,
            "restart" => client
                .restart_container(id, Some(RestartContainerOptions { t: 10 }))
                .await
                .map_err(|e| {
                    HomarrError::Upstream(format!("Docker restart failed on {endpoint_host}: {e}"))
                })?,
            "pause" => client.pause_container(id).await.map_err(|e| {
                HomarrError::Upstream(format!("Docker pause failed on {endpoint_host}: {e}"))
            })?,
            "unpause" => client.unpause_container(id).await.map_err(|e| {
                HomarrError::Upstream(format!("Docker unpause failed on {endpoint_host}: {e}"))
            })?,
            "remove" => client
                .remove_container(
                    id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
                .map_err(|e| {
                    HomarrError::Upstream(format!("Docker remove failed on {endpoint_host}: {e}"))
                })?,
            other => {
                return Err(HomarrError::BadRequest(format!(
                    "Unsupported Docker container action '{other}'"
                )));
            }
        }
        Ok(())
    }

    pub async fn container_logs(&self, id: &str, tail: usize) -> HomarrResult<String> {
        use futures_util::StreamExt;
        let (_endpoint_host, client) = self.find_client_for_container(id).await?;
        let mut stream = client.logs(
            id,
            Some(LogsOptions::<String> {
                stdout: true,
                stderr: true,
                tail: tail.min(2000).to_string(),
                ..Default::default()
            }),
        );
        let mut lines = Vec::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(output) => lines.push(output.to_string()),
                Err(err) => {
                    return Err(HomarrError::Upstream(format!(
                        "Docker log stream error: {err}"
                    )));
                }
            }
        }
        Ok(lines.concat())
    }

    pub async fn discover_services(&self) -> HomarrResult<Vec<DiscoveredDockerService>> {
        let containers = self.list_containers().await?;
        let mut discovered = Vec::new();
        for c in containers {
            if let Some(m) = match_docker_service(&c.image, &c.name) {
                let candidates = build_docker_service_url_candidates(
                    &c.name,
                    &c.endpoint,
                    &c.ports,
                    c.ports.first().map(|p| p.private_port),
                );
                discovered.push(DiscoveredDockerService {
                    container_id: c.id,
                    container_name: c.name,
                    image: c.image,
                    state: c.state,
                    service_match: m,
                    url_candidates: candidates,
                });
            }
        }
        Ok(discovered)
    }

    async fn find_client_for_container(&self, id: &str) -> HomarrResult<(String, Docker)> {
        let clients = self.configured_endpoints();
        for (_eid, endpoint_host, client) in clients {
            if client.inspect_container(id, None).await.is_ok() {
                return Ok((endpoint_host, client));
            }
        }
        Err(HomarrError::NotFound(format!(
            "Container '{id}' not found on any configured Docker endpoint"
        )))
    }
}

fn map_container_summary(endpoint_host: &str, c: ContainerSummary) -> DockerContainerInfo {
    let id = c.id.unwrap_or_default();
    let raw_name = c
        .names
        .as_deref()
        .and_then(|n| n.first())
        .map(String::as_str)
        .unwrap_or(&id);
    let name = raw_name.trim_start_matches('/').to_string();
    let image = c.image.unwrap_or_default();
    let labels = c.labels.unwrap_or_default();
    let ports = c
        .ports
        .unwrap_or_default()
        .into_iter()
        .map(|p| DockerServicePort {
            ip: p.ip,
            private_port: p.private_port,
            public_port: p.public_port,
            port_type: p
                .typ
                .map(|t| t.to_string().to_lowercase())
                .unwrap_or_else(|| "tcp".to_string()),
        })
        .collect();
    let icon_url = match_docker_service(&image, &name).map(|m| {
        homarr_definitions::integration::get_integration_def(m.kind)
            .icon_url
            .to_string()
    });
    DockerContainerInfo {
        id,
        name,
        image,
        state: c.state.unwrap_or_else(|| "unknown".to_string()),
        status: c.status.unwrap_or_default(),
        created: c.created.unwrap_or(0),
        endpoint: endpoint_host.to_string(),
        ports,
        labels,
        icon_url,
    }
}

/// Decodes Docker's 8-byte multiplexed log framing (`[stream_type, 0, 0, 0, be_len32]`)
/// with clean UTF-8 fallback when unframed text is supplied.
pub fn decode_docker_logs(raw: &[u8]) -> String {
    const HEADER_LEN: usize = 8;
    if raw.is_empty() {
        return String::new();
    }

    let mut cursor = 0usize;
    let mut parts = Vec::new();
    while cursor < raw.len() {
        if cursor + HEADER_LEN > raw.len() {
            break;
        }
        let stream_type = raw[cursor];
        if stream_type != 1 && stream_type != 2 && stream_type != 0 {
            return String::from_utf8_lossy(raw).into_owned();
        }
        if raw[cursor + 1] != 0 || raw[cursor + 2] != 0 || raw[cursor + 3] != 0 {
            return String::from_utf8_lossy(raw).into_owned();
        }
        let len = u32::from_be_bytes([
            raw[cursor + 4],
            raw[cursor + 5],
            raw[cursor + 6],
            raw[cursor + 7],
        ]) as usize;
        let start = cursor + HEADER_LEN;
        let end = start.saturating_add(len);
        if end > raw.len() {
            break;
        }
        parts.push(String::from_utf8_lossy(&raw[start..end]).into_owned());
        cursor = end;
    }

    if parts.is_empty() {
        String::from_utf8_lossy(raw).into_owned()
    } else {
        parts.concat()
    }
}
