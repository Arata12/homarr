use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use homarr_auth::{check_integration_permission, AuthSession};
use homarr_common::{HomarrError, HomarrResult, SecretCrypto};
use homarr_core::{AppConfig, HttpClientFactory};
use homarr_db::{repos::custom_widgets as cw_repo, Database};
use homarr_definitions::IntegrationPermission;
use homarr_integrations::{IntegrationContext, IntegrationRegistry};
use regex::Regex;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

const CUSTOM_WIDGET_SCHEMA_V2: &str = "homarr-custom-widget-v2";
const MAX_TEMPLATE_LEN: usize = 50_000;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_MAX_PER_WIDGET: usize = 120;

#[derive(Debug, Default)]
struct RateBucket {
    hits: Vec<Instant>,
}

#[derive(Clone)]
pub struct CustomWidgetService {
    config: AppConfig,
    http: HttpClientFactory,
    limiter: Arc<Mutex<HashMap<String, RateBucket>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopListing {
    pub id: String,
    pub slug: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub author: Option<String>,
    pub downloads: Option<i64>,
    pub version: Option<String>,
}

impl CustomWidgetService {
    pub fn new(config: AppConfig, http: HttpClientFactory) -> Self {
        Self {
            config,
            http,
            limiter: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// `session` is `None` only for options loaded from stored board items; API callers must pass
    /// their session so caller-chosen integrations are permission-checked.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_request(
        &self,
        db: &Database,
        crypto: &SecretCrypto,
        integrations: &IntegrationRegistry,
        session: Option<&AuthSession>,
        definition_id: &str,
        request_id: &str,
        options: &HashMap<String, Value>,
    ) -> HomarrResult<Value> {
        self.check_rate_limit(definition_id).await?;

        let def = cw_repo::get_v2_by_id(db, definition_id)
            .await?
            .ok_or_else(|| {
                HomarrError::NotFound(format!(
                    "Custom widget definition '{definition_id}' not found"
                ))
            })?;
        if !def.enabled {
            return Err(HomarrError::Forbidden(
                "Custom widget definition is disabled".to_string(),
            ));
        }

        let requests = def.requests.as_array().ok_or_else(|| {
            HomarrError::BadRequest("Custom widget requests must be an array".into())
        })?;
        let req_spec = requests
            .iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(request_id))
            .ok_or_else(|| {
                HomarrError::NotFound(format!("Request '{request_id}' not found on custom widget"))
            })?;

        let source_id = req_spec
            .get("sourceId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HomarrError::BadRequest("Request missing sourceId".into()))?;

        let sources = def.sources.as_array().ok_or_else(|| {
            HomarrError::BadRequest("Custom widget sources must be an array".into())
        })?;
        let source_spec = sources
            .iter()
            .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(source_id))
            .ok_or_else(|| HomarrError::NotFound(format!("Source '{source_id}' not found")))?;

        let secrets_rows = cw_repo::list_v2_secrets(db, definition_id).await?;
        let mut secrets = HashMap::new();
        for s in secrets_rows {
            if s.source_id == source_id {
                if let Ok(plain) = crypto.decrypt_secret(&s.encrypted_value) {
                    secrets.insert(s.kind, plain);
                }
            }
        }

        let path_template = req_spec.get("path").and_then(|v| v.as_str()).unwrap_or("/");
        let path = interpolate_options(path_template, options);
        let method = req_spec
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");

        let source_kind = source_spec
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("http");

        if source_kind == "integration" {
            let integration_id = options
                .get("integrationId")
                .and_then(|v| v.as_str())
                .or_else(|| source_spec.get("integrationId").and_then(|v| v.as_str()))
                .ok_or_else(|| HomarrError::BadRequest("Integration ID is required".into()))?;
            // The caller may pick the integration through options, so they must be allowed to use it.
            if let Some(session) = session {
                check_integration_permission(
                    db,
                    session,
                    integration_id,
                    IntegrationPermission::Use,
                )
                .await?;
            }
            let integration = homarr_db::repos::integrations::get_by_id(db, integration_id)
                .await?
                .ok_or_else(|| HomarrError::NotFound("Integration not found".into()))?;
            let raw_secrets =
                homarr_db::repos::integrations::get_secrets(db, integration_id).await?;
            let mut int_secrets = HashMap::new();
            for s in raw_secrets {
                if let Ok(plain) = crypto.decrypt_secret(&s.value) {
                    int_secrets.insert(s.kind, plain);
                }
            }
            let kind = homarr_definitions::IntegrationKind::from_str_opt(&integration.kind)
                .ok_or_else(|| HomarrError::BadRequest("Invalid integration kind".into()))?;
            let ctx = IntegrationContext {
                id: integration.id,
                name: integration.name,
                url: integration.url,
                external_url: integration.external_url,
                kind,
                secrets: int_secrets,
            };
            let resp = integrations
                .execute_http_request(&ctx, method, &path, None, None, None)
                .await?;
            return Ok(json!({ "status": resp.status, "data": resp.body }));
        }

        let base_url = source_spec
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HomarrError::BadRequest("HTTP source missing url".into()))?;
        let full_url = if path.starts_with("http://") || path.starts_with("https://") {
            path
        } else {
            format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        };
        // Options are caller-controlled; never let them move the source's secrets to another origin.
        ensure_same_origin(base_url, &full_url)?;

        let mut headers = HashMap::new();
        if let Some(bearer) = secrets.get("bearerToken") {
            headers.insert("Authorization".to_string(), format!("Bearer {bearer}"));
        }
        if let Some(api_key) = secrets.get("apiKey") {
            headers.insert("X-Api-Key".to_string(), api_key.clone());
        }
        for (k, v) in &secrets {
            if let Some(hdr_name) = k.strip_prefix("header:") {
                headers.insert(hdr_name.to_string(), v.clone());
            }
        }

        if method.eq_ignore_ascii_case("GET") {
            let (bytes, _, status) = self
                .http
                .guarded_get(&full_url, Some(&headers), None, 4 * 1024 * 1024)
                .await?;
            let text = String::from_utf8_lossy(&bytes);
            let parsed = serde_json::from_str::<Value>(&text)
                .unwrap_or_else(|_| Value::String(text.into_owned()));
            return Ok(json!({ "status": status, "data": parsed }));
        }

        let req_method = Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes())
            .map_err(|_| HomarrError::BadRequest(format!("Invalid HTTP method '{method}'")))?;
        let _ = homarr_common::validate_outbound_url(&full_url, None)?;
        let mut req = self.http.guarded().request(req_method, &full_url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(body) = req_spec.get("body") {
            req = req.json(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let parsed = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
        Ok(json!({ "status": status, "data": parsed }))
    }

    pub async fn fetch_workshop_listings(
        &self,
        query: Option<&str>,
    ) -> HomarrResult<Vec<WorkshopListing>> {
        if self.config.no_external_connection {
            return Ok(Vec::new());
        }
        let mut url = format!(
            "{}/api/listings",
            self.config.workshop_api_url.trim_end_matches('/')
        );
        if let Some(q) = query.filter(|s| !s.trim().is_empty()) {
            let encoded: String = url::form_urlencoded::byte_serialize(q.as_bytes()).collect();
            url.push_str(&format!("?q={encoded}"));
        }
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Workshop request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(HomarrError::Upstream(format!(
                "Workshop returned HTTP {}",
                resp.status()
            )));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid Workshop JSON: {e}")))?;
        let arr = body
            .get("items")
            .or_else(|| body.get("listings"))
            .and_then(|v| v.as_array())
            .or_else(|| body.as_array())
            .cloned()
            .unwrap_or_default();
        let listings = arr
            .into_iter()
            .filter_map(|v| serde_json::from_value::<WorkshopListing>(v).ok())
            .collect();
        Ok(listings)
    }

    pub async fn fetch_workshop_definition(&self, listing_id: &str) -> HomarrResult<Value> {
        if self.config.no_external_connection {
            return Err(HomarrError::Forbidden(
                "External connections are disabled (NO_EXTERNAL_CONNECTION=true)".into(),
            ));
        }
        let url = format!(
            "{}/api/listings/{listing_id}/download",
            self.config.workshop_api_url.trim_end_matches('/')
        );
        let resp = self
            .http
            .standard()
            .get(&url)
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Workshop download failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(HomarrError::Upstream(format!(
                "Workshop download returned HTTP {}",
                resp.status()
            )));
        }
        let def: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(format!("Invalid listing definition JSON: {e}")))?;
        validate_custom_widget_definition(&def)?;
        Ok(def)
    }

    async fn check_rate_limit(&self, definition_id: &str) -> HomarrResult<()> {
        let mut guard = self.limiter.lock().await;
        let bucket = guard.entry(definition_id.to_string()).or_default();
        let cutoff = Instant::now() - RATE_LIMIT_WINDOW;
        bucket.hits.retain(|t| *t > cutoff);
        if bucket.hits.len() >= RATE_LIMIT_MAX_PER_WIDGET {
            return Err(HomarrError::BadRequest(
                "Custom widget rate limit exceeded".to_string(),
            ));
        }
        bucket.hits.push(Instant::now());
        Ok(())
    }
}

/// Validates a Custom Widget v2 definition against prototype pollution, embedded credentials,
/// template length caps, and unsafe JSX/HTML script sinks.
pub fn validate_custom_widget_definition(def: &Value) -> HomarrResult<()> {
    let obj = def.as_object().ok_or_else(|| {
        HomarrError::BadRequest("Custom widget definition must be a JSON object".into())
    })?;

    if let Some(schema) = obj.get("schema").and_then(|v| v.as_str()) {
        if schema != CUSTOM_WIDGET_SCHEMA_V2 {
            return Err(HomarrError::BadRequest(format!(
                "Unsupported custom widget schema '{schema}'; expected '{CUSTOM_WIDGET_SCHEMA_V2}'"
            )));
        }
    }

    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        return Err(HomarrError::BadRequest(
            "Custom widget definition must specify a non-empty name".into(),
        ));
    }

    let template = obj.get("template").and_then(|v| v.as_str()).unwrap_or("");
    if template.trim().is_empty() || template.len() > MAX_TEMPLATE_LEN {
        return Err(HomarrError::BadRequest(format!(
            "Custom widget template must be between 1 and {MAX_TEMPLATE_LEN} characters"
        )));
    }

    validate_no_prototype_keys(def)?;
    validate_no_secrets_in_json(def)?;
    validate_template_safety(template)?;

    Ok(())
}

fn validate_no_prototype_keys(value: &Value) -> HomarrResult<()> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if matches!(k.as_str(), "__proto__" | "constructor" | "prototype") {
                    return Err(HomarrError::BadRequest(format!(
                        "Forbidden property key '{k}' in custom widget definition"
                    )));
                }
                validate_no_prototype_keys(v)?;
            }
        }
        Value::Array(arr) => {
            for item in arr {
                validate_no_prototype_keys(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_no_secrets_in_json(value: &Value) -> HomarrResult<()> {
    let secret_pattern = Regex::new(
        r#"(?i)(sk-[a-z0-9]{16,}|ghp_[a-z0-9]{20,}|github_pat_[a-z0-9_]{20,}|xox[baprs]-[a-z0-9-]{10,}|glpat-[a-z0-9_-]{16,}|Bearer\s+[a-z0-9._-]{16,})"#,
    )
    .expect("valid regex");

    match value {
        Value::String(s) => {
            if secret_pattern.is_match(s) {
                return Err(HomarrError::BadRequest(
                    "Custom widget definition must not contain hardcoded API keys or tokens".into(),
                ));
            }
        }
        Value::Array(arr) => {
            for item in arr {
                validate_no_secrets_in_json(item)?;
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                if matches!(
                    k.to_ascii_lowercase().as_str(),
                    "apikey" | "api_key" | "password" | "bearertoken" | "secretvalue"
                ) && v
                    .as_str()
                    .map(|s| !s.trim().is_empty() && !s.contains("{{"))
                    .unwrap_or(false)
                {
                    return Err(HomarrError::BadRequest(format!(
                        "Hardcoded secret key '{k}' must use Custom Widget secret storage instead"
                    )));
                }
                validate_no_secrets_in_json(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_template_safety(template: &str) -> HomarrResult<()> {
    let lower = template.to_ascii_lowercase();
    for forbidden in [
        "<script",
        "javascript:",
        "onerror=",
        "onload=",
        "eval(",
        "function(",
        "document.cookie",
    ] {
        if lower.contains(forbidden) {
            return Err(HomarrError::BadRequest(format!(
                "Custom widget template contains disallowed construct '{forbidden}'"
            )));
        }
    }
    Ok(())
}

/// Renders a Custom Widget template replacing `{{path.to.value}}` placeholders with HTML-escaped data.
pub fn render_template(template: &str, data: &Value, options: &HashMap<String, Value>) -> String {
    let re = Regex::new(r"\{\{\s*([a-zA-Z0-9_.-]+)\s*\}\}").expect("valid placeholder regex");
    re.replace_all(template, |caps: &regex::Captures<'_>| {
        let key = &caps[1];
        if let Some(opt_key) = key.strip_prefix("options.") {
            if let Some(v) = options.get(opt_key) {
                return html_escape::encode_text(&value_to_string(v)).into_owned();
            }
        }
        let lookup_key = key.strip_prefix("data.").unwrap_or(key);
        if let Some(val) = lookup_json_path(data, lookup_key) {
            html_escape::encode_text(&value_to_string(val)).into_owned()
        } else {
            String::new()
        }
    })
    .into_owned()
}

fn ensure_same_origin(base_url: &str, full_url: &str) -> HomarrResult<()> {
    let source = url::Url::parse(base_url)
        .map_err(|e| HomarrError::BadRequest(format!("Invalid source url: {e}")))?;
    let target = url::Url::parse(full_url)
        .map_err(|e| HomarrError::BadRequest(format!("Invalid request url: {e}")))?;
    if source.origin() != target.origin() {
        return Err(HomarrError::Forbidden(
            "Custom widget request must stay on its source origin".into(),
        ));
    }
    Ok(())
}

fn interpolate_options(template: &str, options: &HashMap<String, Value>) -> String {
    let re = Regex::new(r"\{\{\s*options\.([a-zA-Z0-9_.-]+)\s*\}\}").expect("valid regex");
    re.replace_all(template, |caps: &regex::Captures<'_>| {
        let key = &caps[1];
        options.get(key).map(value_to_string).unwrap_or_default()
    })
    .into_owned()
}

fn lookup_json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut curr = root;
    for part in path.split('.') {
        if let Ok(idx) = part.parse::<usize>() {
            curr = curr.as_array()?.get(idx)?;
        } else {
            curr = curr.as_object()?.get(part)?;
        }
    }
    Some(curr)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_allows_paths_on_source() {
        assert!(ensure_same_origin("http://host:8080/api", "http://host:8080/api/x?y=1").is_ok());
    }

    #[test]
    fn same_origin_rejects_other_hosts_ports_and_schemes() {
        for target in [
            "http://evil.example/steal",
            "http://host:9090/api",
            "https://host:8080/api",
            "http://host:8080@evil.example/",
        ] {
            assert!(matches!(
                ensure_same_origin("http://host:8080/api", target),
                Err(HomarrError::Forbidden(_))
            ));
        }
    }

    #[test]
    fn absolute_url_from_options_is_rejected() {
        let mut options = HashMap::new();
        options.insert("p".to_string(), json!("https://evil.example/x"));
        let path = interpolate_options("{{options.p}}", &options);
        assert!(ensure_same_origin("http://host:8080", &path).is_err());
    }
}
