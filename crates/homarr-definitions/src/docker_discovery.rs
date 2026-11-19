use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::integration::{all_integration_defs, get_integration_def, IntegrationKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockerServiceMatchConfidence {
    High,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockerServiceMatchSource {
    Image,
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerServiceMatch {
    pub kind: IntegrationKind,
    pub confidence: DockerServiceMatchConfidence,
    pub source: DockerServiceMatchSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerServicePort {
    pub ip: Option<String>,
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub port_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerServiceUrlCandidate {
    pub id: String,
    pub url: String,
    pub rank: u8,
    pub source: String,
    pub scopes: Vec<String>,
    pub reason: String,
}

pub fn extract_container_image_name(image: &str) -> String {
    let last = image.rsplit('/').next().unwrap_or("");
    last.split(':').next().unwrap_or("").to_string()
}

fn extract_icon_slug(icon_url: &str) -> String {
    let filename = icon_url.rsplit('/').next().unwrap_or("");
    filename
        .trim_end_matches(".svg")
        .trim_end_matches(".png")
        .to_lowercase()
}

pub fn match_integration_kind(search: &str) -> Option<IntegrationKind> {
    let normalized = search.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let defs = all_integration_defs();
    for d in defs {
        if d.kind_str.to_lowercase() == normalized || d.name.to_lowercase() == normalized {
            return Some(d.kind);
        }
    }
    for d in defs {
        if extract_icon_slug(d.icon_url) == normalized {
            return Some(d.kind);
        }
    }
    for d in defs {
        if d.docker_aliases.iter().any(|a| *a == normalized) {
            return Some(d.kind);
        }
    }
    for d in defs {
        let slug = extract_icon_slug(d.icon_url);
        if normalized.contains(&d.kind_str.to_lowercase())
            || (!slug.is_empty() && normalized.contains(&slug))
        {
            return Some(d.kind);
        }
    }
    None
}

pub fn match_integration_kind_from_container(image: &str, name: &str) -> Option<IntegrationKind> {
    let image_name = extract_container_image_name(image);
    if let Some(kind) = match_integration_kind(&image_name) {
        if get_integration_def(kind).docker_discoverable {
            return Some(kind);
        }
    }
    if let Some(kind) = match_integration_kind(name) {
        if get_integration_def(kind).docker_discoverable {
            return Some(kind);
        }
    }
    None
}

pub fn match_docker_service(image: &str, name: &str) -> Option<DockerServiceMatch> {
    let kind = match_integration_kind_from_container(image, name)?;
    let image_name = extract_container_image_name(image);
    let source = if match_integration_kind(&image_name) == Some(kind) {
        DockerServiceMatchSource::Image
    } else {
        DockerServiceMatchSource::Name
    };
    let confidence = match source {
        DockerServiceMatchSource::Image => DockerServiceMatchConfidence::High,
        DockerServiceMatchSource::Name => DockerServiceMatchConfidence::Medium,
    };
    Some(DockerServiceMatch {
        kind,
        confidence,
        source,
    })
}

pub fn normalize_docker_service_url(value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    match Url::parse(raw) {
        Ok(mut url) => {
            url.set_fragment(None);
            Some(url.to_string().trim_end_matches('/').to_lowercase())
        }
        Err(_) => {
            let trimmed = raw.trim_end_matches('/').to_lowercase();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
    }
}

fn is_usable_published_address(value: Option<&str>) -> bool {
    let Some(raw) = value else {
        return false;
    };
    let host = raw
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    !matches!(
        host.as_str(),
        "0.0.0.0" | "::" | "::0" | "::1" | "localhost"
    ) && !host.ends_with(".localhost")
        && !host.starts_with("127.")
        && !host.starts_with("::ffff:127.")
}

fn get_endpoint_hostname(host: &str) -> Option<String> {
    if host == "socket" || host.starts_with('/') || host.contains("://") {
        return None;
    }
    Url::parse(&format!("http://{host}"))
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
}

fn format_url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

pub fn build_docker_service_url_candidates(
    container_name: &str,
    endpoint_host: &str,
    ports: &[DockerServicePort],
    preferred_port: Option<u16>,
) -> Vec<DockerServiceUrlCandidate> {
    let mut candidates: Vec<DockerServiceUrlCandidate> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let endpoint_hostname = get_endpoint_hostname(endpoint_host);

    let mut tcp_ports: Vec<&DockerServicePort> = ports
        .iter()
        .filter(|p| p.port_type.eq_ignore_ascii_case("tcp"))
        .collect();
    tcp_ports.sort_by_key(|p| u8::from(Some(p.private_port) != preferred_port));

    let mut add = |url: String, rank: u8, source: &str, scopes: Vec<&str>, reason: &str| {
        let scopes_vec: Vec<String> = scopes.into_iter().map(str::to_string).collect();
        let scopes_key = scopes_vec.join("-");
        if !seen.insert((url.clone(), scopes_key.clone())) {
            return;
        }
        let url_part = if url.is_empty() {
            "manual"
        } else {
            url.as_str()
        };
        let id = format!("{source}:{scopes_key}:{url_part}");
        candidates.push(DockerServiceUrlCandidate {
            id,
            url,
            rank,
            source: source.to_string(),
            scopes: scopes_vec,
            reason: reason.to_string(),
        });
    };

    for port in tcp_ports {
        if let Some(public_port) = port.public_port {
            let published_host = if is_usable_published_address(port.ip.as_deref()) {
                port.ip.clone()
            } else if is_usable_published_address(endpoint_hostname.as_deref()) {
                endpoint_hostname.clone()
            } else {
                None
            };
            if let Some(host) = published_host {
                let from_ip = is_usable_published_address(port.ip.as_deref());
                add(
                    format!("http://{}:{public_port}", format_url_host(&host)),
                    if Some(port.private_port) == preferred_port {
                        1
                    } else {
                        2
                    },
                    if from_ip {
                        "publishedAddress"
                    } else {
                        "endpointHost"
                    },
                    vec!["browser", "server"],
                    if from_ip {
                        "publishedAddress"
                    } else {
                        "dockerEndpointHost"
                    },
                );
            }
        }

        add(
            format!("http://{container_name}:{}", port.private_port),
            if Some(port.private_port) == preferred_port {
                3
            } else {
                4
            },
            "containerDns",
            vec!["server"],
            "sharedContainerNetwork",
        );
    }

    add(
        String::new(),
        99,
        "manual",
        vec!["browser"],
        "manualHostRequired",
    );

    candidates.sort_by_key(|c| c.rank);
    candidates
}
