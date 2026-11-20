use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

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
    #[serde(default = "all_capabilities")]
    capabilities: Vec<DockerCapability>,
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
        #[serde(rename = "caPath")]
        ca_path: String,
        #[serde(rename = "certPath", default)]
        cert_path: Option<String>,
        #[serde(rename = "keyPath", default)]
        key_path: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DockerCapability {
    Inventory,
    Logs,
    Lifecycle,
    Remove,
}

fn all_capabilities() -> Vec<DockerCapability> {
    vec![
        DockerCapability::Inventory,
        DockerCapability::Logs,
        DockerCapability::Lifecycle,
        DockerCapability::Remove,
    ]
}

fn require_capability(
    capabilities: &[DockerCapability],
    needed: DockerCapability,
) -> HomarrResult<()> {
    if capabilities.contains(&needed) {
        return Ok(());
    }
    let name = format!("{needed:?}").to_lowercase();
    Err(HomarrError::Forbidden(format!(
        "Docker endpoint does not permit {name} access"
    )))
}

/// Mirrors `parseDockerEndpointDescriptors`: an invalid DOCKER_ENDPOINTS is an error, never a fallback.
fn parse_endpoint_specs(raw: &str) -> HomarrResult<Vec<EnvEndpointSpec>> {
    let invalid = |msg: String| HomarrError::Internal(format!("Invalid DOCKER_ENDPOINTS: {msg}"));
    let specs: Vec<EnvEndpointSpec> =
        serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
    if specs.is_empty() {
        return Err(invalid("must be a non-empty JSON array".to_string()));
    }
    let unique_ids: std::collections::HashSet<&str> =
        specs.iter().map(|spec| spec.id.as_str()).collect();
    if unique_ids.len() != specs.len() {
        return Err(invalid("ids must be unique".to_string()));
    }
    for (index, spec) in specs.iter().enumerate() {
        if !spec.capabilities.contains(&DockerCapability::Inventory) {
            return Err(invalid(format!(
                "[{index}].capabilities must include inventory"
            )));
        }
        match &spec.transport {
            EnvEndpointTransport::Tcp {
                allow_insecure: false,
                ..
            } => {
                return Err(invalid(format!(
                    "[{index}] plaintext TCP requires allowInsecure: true"
                )));
            }
            EnvEndpointTransport::Tls {
                cert_path,
                key_path,
                ..
            } if cert_path.is_some() != key_path.is_some() => {
                return Err(invalid(format!(
                    "[{index}] TLS certPath and keyPath must be provided together"
                )));
            }
            _ => {}
        }
    }
    Ok(specs)
}

/// Docker Engine API over HTTPS with the endpoint's CA and optional client certificate.
#[derive(Clone)]
struct TlsDockerClient {
    http: reqwest::Client,
    base: String,
}

impl TlsDockerClient {
    fn connect(
        host: &str,
        port: u16,
        ca_path: &str,
        cert_path: Option<&str>,
        key_path: Option<&str>,
    ) -> HomarrResult<Self> {
        let read = |path: &str| {
            std::fs::read(path).map_err(|e| {
                HomarrError::Internal(format!("Failed to read Docker TLS file {path}: {e}"))
            })
        };
        let tls_err =
            |e: reqwest::Error| HomarrError::Internal(format!("Docker TLS setup failed: {e}"));
        let ca = reqwest::Certificate::from_pem(&read(ca_path)?).map_err(tls_err)?;
        let mut builder = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca)
            .timeout(Duration::from_secs(120));
        if let (Some(cert_path), Some(key_path)) = (cert_path, key_path) {
            let mut pem = read(cert_path)?;
            pem.push(b'\n');
            pem.extend(read(key_path)?);
            builder = builder.identity(reqwest::Identity::from_pem(&pem).map_err(tls_err)?);
        }
        Ok(Self {
            http: builder.build().map_err(tls_err)?,
            base: format!("https://{host}:{port}"),
        })
    }

    fn container_url(&self, id: &str, suffix: &str) -> String {
        let id: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        format!("{}/containers/{id}{suffix}", self.base)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> HomarrResult<reqwest::Response> {
        request
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| {
                HomarrError::Upstream(format!("Docker request to {} failed: {e}", self.base))
            })
    }

    async fn list(&self) -> HomarrResult<Vec<ContainerSummary>> {
        self.send(
            self.http
                .get(format!("{}/containers/json?all=true", self.base)),
        )
        .await?
        .json()
        .await
        .map_err(|e| {
            HomarrError::Upstream(format!("Invalid Docker response from {}: {e}", self.base))
        })
    }

    async fn has_container(&self, id: &str) -> bool {
        self.http
            .get(self.container_url(id, "/json"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    async fn action(&self, id: &str, action: &str) -> HomarrResult<()> {
        let request = match action {
            "start" => self.http.post(self.container_url(id, "/start")),
            "stop" => self.http.post(self.container_url(id, "/stop?t=10")),
            "restart" => self.http.post(self.container_url(id, "/restart?t=10")),
            "pause" => self.http.post(self.container_url(id, "/pause")),
            "unpause" => self.http.post(self.container_url(id, "/unpause")),
            "remove" => self.http.delete(self.container_url(id, "?force=true")),
            other => {
                return Err(HomarrError::BadRequest(format!(
                    "Unsupported Docker container action '{other}'"
                )));
            }
        };
        self.send(request).await.map(|_| ())
    }

    async fn logs(&self, id: &str, tail: usize) -> HomarrResult<String> {
        let url = self.container_url(id, &format!("/logs?stdout=true&stderr=true&tail={tail}"));
        let bytes = self
            .send(self.http.get(url))
            .await?
            .bytes()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Docker log stream error: {e}")))?;
        Ok(decode_docker_logs(&bytes))
    }
}

enum EndpointClient {
    Bollard(Docker),
    Tls(TlsDockerClient),
}

impl EndpointClient {
    async fn list(&self) -> Result<Vec<ContainerSummary>, String> {
        match self {
            Self::Bollard(client) => client
                .list_containers(Some(ListContainersOptions::<String> {
                    all: true,
                    ..Default::default()
                }))
                .await
                .map_err(|e| e.to_string()),
            Self::Tls(client) => client.list().await.map_err(|e| e.to_string()),
        }
    }

    async fn has_container(&self, id: &str) -> bool {
        match self {
            Self::Bollard(client) => client.inspect_container(id, None).await.is_ok(),
            Self::Tls(client) => client.has_container(id).await,
        }
    }
}

struct DockerEndpoint {
    host: String,
    capabilities: Vec<DockerCapability>,
    client: EndpointClient,
}

#[derive(Clone)]
pub struct DockerService {
    config: AppConfig,
}

impl DockerService {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    fn configured_endpoints(&self) -> HomarrResult<Vec<DockerEndpoint>> {
        let mut endpoints = Vec::new();

        let raw = std::env::var("DOCKER_ENDPOINTS").unwrap_or_default();
        if !raw.trim().is_empty() {
            let specs = parse_endpoint_specs(raw.trim()).inspect_err(
                |err| tracing::error!(error = %err, "Failed to parse DOCKER_ENDPOINTS"),
            )?;
            for spec in specs {
                let (host, client) = match spec.transport {
                    EnvEndpointTransport::Socket { path } => {
                        if !Path::new(&path).exists() {
                            continue;
                        }
                        match Docker::connect_with_socket(&path, 120, API_DEFAULT_VERSION) {
                            Ok(client) => (spec.name, EndpointClient::Bollard(client)),
                            Err(_) => continue,
                        }
                    }
                    EnvEndpointTransport::Tcp { host, port, .. } => {
                        let addr = format!("http://{host}:{port}");
                        match Docker::connect_with_http(&addr, 120, API_DEFAULT_VERSION) {
                            Ok(client) => (host, EndpointClient::Bollard(client)),
                            Err(_) => continue,
                        }
                    }
                    EnvEndpointTransport::Tls {
                        host,
                        port,
                        ca_path,
                        cert_path,
                        key_path,
                    } => match TlsDockerClient::connect(
                        &host,
                        port,
                        &ca_path,
                        cert_path.as_deref(),
                        key_path.as_deref(),
                    ) {
                        Ok(client) => (host, EndpointClient::Tls(client)),
                        Err(err) => {
                            tracing::warn!(endpoint = %spec.id, error = %err, "Failed to configure Docker TLS endpoint");
                            continue;
                        }
                    },
                };
                endpoints.push(DockerEndpoint {
                    host,
                    capabilities: spec.capabilities,
                    client,
                });
            }
            return Ok(endpoints);
        }

        for socket_path in &self.config.docker_socket_paths {
            if Path::new(socket_path).exists() {
                if let Ok(client) =
                    Docker::connect_with_socket(socket_path, 120, API_DEFAULT_VERSION)
                {
                    endpoints.push(DockerEndpoint {
                        host: "socket".to_string(),
                        capabilities: all_capabilities(),
                        client: EndpointClient::Bollard(client),
                    });
                }
            }
        }

        for (idx, host) in self.config.docker_hostnames.iter().enumerate() {
            let port = self.config.docker_ports.get(idx).copied().unwrap_or(2375);
            let addr = format!("http://{host}:{port}");
            if let Ok(client) = Docker::connect_with_http(&addr, 120, API_DEFAULT_VERSION) {
                endpoints.push(DockerEndpoint {
                    host: host.clone(),
                    capabilities: all_capabilities(),
                    client: EndpointClient::Bollard(client),
                });
            }
        }

        Ok(endpoints)
    }

    pub async fn list_containers(&self) -> HomarrResult<Vec<DockerContainerInfo>> {
        let mut all = Vec::new();
        for endpoint in self.configured_endpoints()? {
            if !endpoint.capabilities.contains(&DockerCapability::Inventory) {
                continue;
            }
            match endpoint.client.list().await {
                Ok(list) => {
                    for c in list {
                        all.push(map_container_summary(&endpoint.host, c));
                    }
                }
                Err(err) => {
                    tracing::warn!(endpoint = %endpoint.host, error = %err, "Failed to list Docker containers");
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
        if !matches!(
            action,
            "start" | "stop" | "restart" | "pause" | "unpause" | "remove"
        ) {
            return Err(HomarrError::BadRequest(format!(
                "Unsupported Docker container action '{action}'"
            )));
        }
        let capability = if action == "remove" {
            DockerCapability::Remove
        } else {
            DockerCapability::Lifecycle
        };
        let endpoint = self.find_endpoint_for_container(id, capability).await?;
        let endpoint_host = &endpoint.host;
        let client = match &endpoint.client {
            EndpointClient::Bollard(client) => client,
            EndpointClient::Tls(client) => return client.action(id, action).await,
        };
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
        let endpoint = self
            .find_endpoint_for_container(id, DockerCapability::Logs)
            .await?;
        let client = match &endpoint.client {
            EndpointClient::Bollard(client) => client,
            EndpointClient::Tls(client) => return client.logs(id, tail.min(2000)).await,
        };
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

    /// Like the original, only endpoints granting `needed` may act; a match elsewhere is Forbidden.
    async fn find_endpoint_for_container(
        &self,
        id: &str,
        needed: DockerCapability,
    ) -> HomarrResult<DockerEndpoint> {
        let mut denied = None;
        for endpoint in self.configured_endpoints()? {
            if endpoint.client.has_container(id).await {
                match require_capability(&endpoint.capabilities, needed) {
                    Ok(()) => return Ok(endpoint),
                    Err(err) => denied = Some(err),
                }
            }
        }
        Err(denied.unwrap_or_else(|| {
            HomarrError::NotFound(format!(
                "Container '{id}' not found on any configured Docker endpoint"
            ))
        }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_capabilities_are_parsed_and_enforced() {
        let specs = parse_endpoint_specs(
            r#"[{"id":"a","name":"A","transport":{"type":"socket","path":"/run/docker.sock"},"capabilities":["inventory","logs"]},
                {"id":"b","name":"B","transport":{"type":"socket","path":"/run/podman.sock"}}]"#,
        )
        .unwrap();
        assert!(require_capability(&specs[0].capabilities, DockerCapability::Logs).is_ok());
        assert!(matches!(
            require_capability(&specs[0].capabilities, DockerCapability::Remove),
            Err(HomarrError::Forbidden(_))
        ));
        assert_eq!(specs[1].capabilities, all_capabilities());

        let socket = r#""transport":{"type":"socket","path":"/s"}"#;
        for bad in [
            "not json".to_string(),
            "[]".to_string(),
            format!(r#"[{{"id":"a","name":"A",{socket},"capabilities":["logs"]}}]"#),
            format!(r#"[{{"id":"a","name":"A",{socket},"capabilities":["inventory","root"]}}]"#),
            r#"[{"id":"a","name":"A","transport":{"type":"tcp","host":"h","port":2375}}]"#
                .to_string(),
        ] {
            assert!(parse_endpoint_specs(&bad).is_err(), "{bad}");
        }
    }
}
