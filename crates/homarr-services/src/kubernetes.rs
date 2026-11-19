use homarr_common::{HomarrError, HomarrResult};
use homarr_core::{AppConfig, HttpClientFactory};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct KubernetesService {
    config: AppConfig,
    http: HttpClientFactory,
}

impl KubernetesService {
    pub fn new(config: AppConfig, http: HttpClientFactory) -> Self {
        Self { config, http }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enable_kubernetes
    }

    pub async fn get_resource(&self, resource: &str) -> HomarrResult<Value> {
        if !self.config.enable_kubernetes {
            return Err(HomarrError::Forbidden(
                "Kubernetes integration is disabled (ENABLE_KUBERNETES=false)".to_string(),
            ));
        }

        let path = match resource {
            "nodes" => "/api/v1/nodes",
            "namespaces" => "/api/v1/namespaces",
            "pods" => "/api/v1/pods",
            "ingresses" => "/apis/networking.k8s.io/v1/ingresses",
            "volumes" => "/api/v1/persistentvolumeclaims",
            "secrets" => "/api/v1/secrets",
            other => {
                return Err(HomarrError::BadRequest(format!(
                    "Unknown Kubernetes resource '{other}'"
                )));
            }
        };

        let host = std::env::var("KUBERNETES_SERVICE_HOST")
            .map_err(|_| HomarrError::Upstream("KUBERNETES_SERVICE_HOST is not set".to_string()))?;
        let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".to_string());
        let token =
            tokio::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
                .await
                .map_err(|e| {
                    HomarrError::Upstream(format!(
                        "Unable to read in-cluster service account token: {e}"
                    ))
                })?;

        let url = format!("https://{host}:{port}{path}");
        let ca_path = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";
        let client = if let Ok(pem) = tokio::fs::read(ca_path).await {
            if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                Client::builder()
                    .add_root_certificate(cert)
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .unwrap_or_else(|_| self.http.standard())
            } else {
                self.http.standard()
            }
        } else {
            self.http.standard()
        };

        let resp = client
            .get(&url)
            .bearer_auth(token.trim())
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Kubernetes API error: {e}")))?;
        let status = resp.status();
        let data: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Kubernetes JSON decode error: {e}")))?;
        if !status.is_success() {
            return Err(HomarrError::Upstream(format!(
                "Kubernetes API returned HTTP {status}"
            )));
        }

        Ok(project_resource(resource, &data))
    }
}

fn str_at<'a>(item: &'a Value, pointer: &str, default: &'a str) -> &'a str {
    item.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or(default)
}

fn value_at(item: &Value, pointer: &str) -> Value {
    item.pointer(pointer).cloned().unwrap_or(Value::Null)
}

fn array_at<'a>(item: &'a Value, pointer: &str) -> &'a [Value] {
    item.pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// Keeps only the fields upstream Homarr returns per resource, so raw specs, env values,
/// annotations (e.g. last-applied-configuration) and secret data never leave the server.
fn project_resource(resource: &str, data: &Value) -> Value {
    let projected: Vec<Value> = array_at(data, "/items")
        .iter()
        .map(|item| {
            let name = str_at(item, "/metadata/name", "unknown");
            let namespace = str_at(item, "/metadata/namespace", "unknown");
            let created = value_at(item, "/metadata/creationTimestamp");
            match resource {
                "nodes" => {
                    let ready = array_at(item, "/status/conditions")
                        .iter()
                        .find(|c| c.get("type").and_then(Value::as_str) == Some("Ready"))
                        .and_then(|c| c.get("status"))
                        .and_then(Value::as_str)
                        == Some("True");
                    let status = if ready { "Ready" } else { "NotReady" };
                    let pods_count = item
                        .pointer("/status/capacity/pods")
                        .and_then(Value::as_str)
                        .and_then(|p| p.parse::<u64>().ok());
                    json!({
                        "name": name,
                        "status": status,
                        // No metrics-server lookup in this port; upstream also returns null without metrics.
                        "allocatableCpuPercentage": null,
                        "allocatableRamPercentage": null,
                        "podsCount": pods_count,
                        "operatingSystem": value_at(item, "/status/nodeInfo/operatingSystem"),
                        "architecture": value_at(item, "/status/nodeInfo/architecture"),
                        "kubernetesVersion": value_at(item, "/status/nodeInfo/kubeletVersion"),
                        "creationTimestamp": created,
                    })
                }
                "namespaces" => json!({
                    "status": value_at(item, "/status/phase"),
                    "name": name,
                    "creationTimestamp": created,
                }),
                "pods" => {
                    let image = item.pointer("/spec/containers").map(|_| {
                        array_at(item, "/spec/containers")
                            .iter()
                            .map(|c| str_at(c, "/image", ""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                    json!({
                        "name": str_at(item, "/metadata/name", ""),
                        "namespace": str_at(item, "/metadata/namespace", ""),
                        "image": image,
                        "applicationType": pod_application_type(item),
                        "status": str_at(item, "/status/phase", "unknown"),
                        "creationTimestamp": created,
                    })
                }
                "ingresses" => {
                    let rules: Vec<Value> = array_at(item, "/spec/rules")
                        .iter()
                        .map(|rule| {
                            let paths: Vec<Value> = array_at(rule, "/http/paths")
                                .iter()
                                .map(|p| {
                                    json!({
                                        "serviceName": str_at(p, "/backend/service/name", ""),
                                        "port": p
                                            .pointer("/backend/service/port/number")
                                            .and_then(Value::as_u64)
                                            .unwrap_or(0),
                                    })
                                })
                                .collect();
                            json!({ "host": str_at(rule, "/host", ""), "paths": paths })
                        })
                        .collect();
                    json!({
                        "name": str_at(item, "/metadata/name", ""),
                        "namespace": str_at(item, "/metadata/namespace", ""),
                        "className": str_at(item, "/spec/ingressClassName", ""),
                        "rulesAndPaths": rules,
                        "creationTimestamp": created,
                    })
                }
                "volumes" => json!({
                    "name": name,
                    "namespace": namespace,
                    "accessModes": array_at(item, "/status/accessModes"),
                    "storage": str_at(item, "/status/capacity/storage", ""),
                    "storageClassName": str_at(item, "/spec/storageClassName", ""),
                    "volumeMode": str_at(item, "/spec/volumeMode", ""),
                    "volumeName": str_at(item, "/spec/volumeName", ""),
                    "status": str_at(item, "/status/phase", ""),
                    "creationTimestamp": created,
                }),
                "secrets" => json!({
                    "name": name,
                    "namespace": namespace,
                    "type": str_at(item, "/type", "unknown"),
                    "creationTimestamp": created,
                }),
                // get_resource rejects unknown resources; never echo raw objects regardless.
                _ => json!({ "name": name, "namespace": namespace }),
            }
        })
        .collect();
    json!({ "items": projected })
}

fn pod_application_type(pod: &Value) -> &'static str {
    if str_at(pod, "/metadata/labels/app.kubernetes.io~1managed-by", "") == "Helm" {
        return "Helm";
    }
    for owner in array_at(pod, "/metadata/ownerReferences") {
        match str_at(owner, "/kind", "") {
            "Deployment" => return "Deployment",
            "StatefulSet" => return "StatefulSet",
            "DaemonSet" => return "DaemonSet",
            // shortcut: upstream reads the ReplicaSet's owner via the API; the Deployment controller
            // always sets pod-template-hash, so use that. Upgrade if bare ReplicaSets get mislabelled.
            "ReplicaSet" => {
                return if pod.pointer("/metadata/labels/pod-template-hash").is_some() {
                    "Deployment"
                } else {
                    "ReplicaSet"
                };
            }
            _ => {}
        }
    }
    "Pod"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_drop_annotations_and_data() {
        let raw = json!({ "items": [{
            "metadata": {
                "name": "db",
                "namespace": "prod",
                "creationTimestamp": "2024-01-01T00:00:00Z",
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{\"data\":{\"pw\":\"c2VjcmV0\"}}"
                }
            },
            "type": "Opaque",
            "data": { "pw": "c2VjcmV0" }
        }]});
        let out = project_resource("secrets", &raw);
        assert_eq!(
            out,
            json!({ "items": [{
                "name": "db",
                "namespace": "prod",
                "type": "Opaque",
                "creationTimestamp": "2024-01-01T00:00:00Z"
            }]})
        );
        assert!(!out.to_string().contains("c2VjcmV0"));
    }

    #[test]
    fn pods_drop_spec_env_values() {
        let raw = json!({ "items": [{
            "metadata": {
                "name": "web-abc",
                "namespace": "default",
                "labels": { "pod-template-hash": "abc" },
                "ownerReferences": [{ "kind": "ReplicaSet", "name": "web-abc" }]
            },
            "spec": { "containers": [
                { "image": "nginx:1", "env": [{ "name": "TOKEN", "value": "hunter2" }] },
                { "image": "redis:7" }
            ]},
            "status": { "phase": "Running" }
        }]});
        let out = project_resource("pods", &raw);
        assert_eq!(
            out,
            json!({ "items": [{
                "name": "web-abc",
                "namespace": "default",
                "image": "nginx:1, redis:7",
                "applicationType": "Deployment",
                "status": "Running",
                "creationTimestamp": null
            }]})
        );
        assert!(!out.to_string().contains("hunter2"));
    }

    #[test]
    fn missing_items_never_echoes_raw_payload() {
        let raw = json!({ "kind": "Status", "data": { "pw": "c2VjcmV0" } });
        assert_eq!(project_resource("secrets", &raw), json!({ "items": [] }));
    }
}
