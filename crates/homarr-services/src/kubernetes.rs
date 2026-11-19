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
            "volumes" => "/api/v1/persistentvolumes",
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
                    .unwrap_or_else(|_| self.http.standard().clone())
            } else {
                self.http.standard().clone()
            }
        } else {
            self.http.standard().clone()
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

        // Redact secret payloads so raw secret data values never leak via API.
        if resource == "secrets" {
            if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
                let redacted: Vec<Value> = items
                    .iter()
                    .map(|item| {
                        let meta = item.get("metadata").cloned().unwrap_or(Value::Null);
                        let kind = item.get("type").cloned().unwrap_or(Value::Null);
                        let keys: Vec<String> = item
                            .get("data")
                            .and_then(|d| d.as_object())
                            .map(|o| o.keys().cloned().collect())
                            .unwrap_or_default();
                        json!({
                            "metadata": meta,
                            "type": kind,
                            "dataKeys": keys
                        })
                    })
                    .collect();
                return Ok(json!({ "items": redacted }));
            }
        }

        Ok(data)
    }
}
