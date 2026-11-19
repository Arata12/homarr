use std::collections::{HashMap, HashSet};

use base64::{
    engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD},
    Engine,
};
use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::integration::{
    get_integration_def, IntegrationHttpAuthDefinition, IntegrationHttpBodyAuth,
    IntegrationHttpStandardAuth,
};
use homarr_definitions::IntegrationKind;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;

use crate::types::IntegrationContext;

#[derive(Debug, Clone, Default)]
pub struct ResolvedHttpAuth {
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: Option<IntegrationHttpBodyAuth>,
    pub redact_values: Vec<String>,
}

pub fn resolve_standard_http_auth(
    ctx: &IntegrationContext,
    std_auth: &IntegrationHttpStandardAuth,
) -> HomarrResult<ResolvedHttpAuth> {
    match std_auth {
        IntegrationHttpStandardAuth::None => Ok(ResolvedHttpAuth::default()),
        IntegrationHttpStandardAuth::Bearer { secret_kind } => {
            let key = secret_kind.map(|k| k.as_str()).unwrap_or("apiKey");
            let token = ctx.require_secret(key)?;
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {token}"));
            Ok(ResolvedHttpAuth {
                headers,
                redact_values: vec![token.to_string()],
                ..Default::default()
            })
        }
        IntegrationHttpStandardAuth::ApiKeyHeader {
            name,
            prefix,
            secret_kind,
        } => {
            let key = secret_kind.map(|k| k.as_str()).unwrap_or("apiKey");
            let token = ctx.require_secret(key)?;
            let val = format!("{}{token}", prefix.unwrap_or(""));
            let mut headers = HashMap::new();
            headers.insert((*name).to_string(), val);
            Ok(ResolvedHttpAuth {
                headers,
                redact_values: vec![token.to_string()],
                ..Default::default()
            })
        }
        IntegrationHttpStandardAuth::ApiKeyQuery { name, secret_kind } => {
            let key = secret_kind.map(|k| k.as_str()).unwrap_or("apiKey");
            let token = ctx.require_secret(key)?;
            let mut query = HashMap::new();
            query.insert((*name).to_string(), token.to_string());
            Ok(ResolvedHttpAuth {
                query,
                redact_values: vec![token.to_string()],
                ..Default::default()
            })
        }
        IntegrationHttpStandardAuth::Basic {
            username_kind,
            username,
            password_kind,
        } => {
            let user_val = match username {
                Some(fixed) => fixed.to_string(),
                None => {
                    let uk = username_kind.map(|k| k.as_str()).unwrap_or("username");
                    ctx.require_secret(uk)?.to_string()
                }
            };
            let pk = password_kind.map(|k| k.as_str()).unwrap_or("password");
            let pass_val = ctx.require_secret(pk)?;
            let raw = format!("{user_val}:{pass_val}");
            let encoded = B64.encode(raw.as_bytes());
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Basic {encoded}"));
            Ok(ResolvedHttpAuth {
                headers,
                redact_values: vec![encoded, pass_val.to_string()],
                ..Default::default()
            })
        }
    }
}

pub fn resolve_declared_http_auth(ctx: &IntegrationContext) -> HomarrResult<ResolvedHttpAuth> {
    let def = get_integration_def(ctx.kind);
    match &def.http_auth {
        IntegrationHttpAuthDefinition::None => Ok(ResolvedHttpAuth::default()),
        IntegrationHttpAuthDefinition::Bearer { secret_kind } => resolve_standard_http_auth(
            ctx,
            &IntegrationHttpStandardAuth::Bearer {
                secret_kind: *secret_kind,
            },
        ),
        IntegrationHttpAuthDefinition::ApiKeyHeader {
            name,
            prefix,
            secret_kind,
        } => resolve_standard_http_auth(
            ctx,
            &IntegrationHttpStandardAuth::ApiKeyHeader {
                name,
                prefix: *prefix,
                secret_kind: *secret_kind,
            },
        ),
        IntegrationHttpAuthDefinition::ApiKeyQuery { name, secret_kind } => {
            resolve_standard_http_auth(
                ctx,
                &IntegrationHttpStandardAuth::ApiKeyQuery {
                    name,
                    secret_kind: *secret_kind,
                },
            )
        }
        IntegrationHttpAuthDefinition::Basic {
            username_kind,
            username,
            password_kind,
        } => resolve_standard_http_auth(
            ctx,
            &IntegrationHttpStandardAuth::Basic {
                username_kind: *username_kind,
                username: *username,
                password_kind: *password_kind,
            },
        ),
        IntegrationHttpAuthDefinition::Modes { modes } => {
            let saved: HashSet<&str> = ctx.secrets.keys().map(String::as_str).collect();
            let matched = modes
                .iter()
                .find(|m| {
                    m.when.len() == saved.len() && m.when.iter().all(|k| saved.contains(k.as_str()))
                })
                .ok_or_else(|| {
                    HomarrError::BadRequest(
                        "Saved credentials do not support generic HTTP requests".to_string(),
                    )
                })?;
            resolve_standard_http_auth(ctx, &matched.auth)
        }
        IntegrationHttpAuthDefinition::Adapter => resolve_adapter_http_auth(ctx),
    }
}

fn resolve_adapter_http_auth(ctx: &IntegrationContext) -> HomarrResult<ResolvedHttpAuth> {
    let mut headers = HashMap::new();
    let mut redact_values = Vec::new();
    match ctx.kind {
        IntegrationKind::Sonarr
        | IntegrationKind::Radarr
        | IntegrationKind::Lidarr
        | IntegrationKind::Readarr
        | IntegrationKind::Sportarr
        | IntegrationKind::Prowlarr
        | IntegrationKind::Overseerr
        | IntegrationKind::Jellyseerr
        | IntegrationKind::Seerr => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("X-Api-Key".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::Proxmox => {
            let user = ctx.require_secret("username")?;
            let realm = ctx.require_secret("realm")?;
            let token_id = ctx.require_secret("tokenId")?;
            let secret = ctx.require_secret("apiKey")?;
            headers.insert(
                "Authorization".to_string(),
                format!("PVEAPIToken={user}@{realm}!{token_id}={secret}"),
            );
            redact_values.push(secret.to_string());
        }
        IntegrationKind::HomeAssistant
        | IntegrationKind::Coolify
        | IntegrationKind::Mealie
        | IntegrationKind::SpeedtestTracker
        | IntegrationKind::Audiobookshelf
        | IntegrationKind::Karakeep
        | IntegrationKind::Netalertx => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("Authorization".to_string(), format!("Bearer {key}"));
            redact_values.push(key.to_string());
        }
        IntegrationKind::Jellystat => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("X-API-Token".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::Tubearchivist => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("Authorization".to_string(), format!("Token {key}"));
            redact_values.push(key.to_string());
        }
        IntegrationKind::Changedetection => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("x-api-key".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::Stash => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("ApiKey".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::Homebox => {
            if let Some(key) = ctx.secret("apiKey") {
                headers.insert("Authorization".to_string(), format!("Bearer {key}"));
                redact_values.push(key.to_string());
            }
        }
        IntegrationKind::Umami => {
            if let Some(key) = ctx.secret("apiKey") {
                headers.insert("x-umami-api-key".to_string(), key.to_string());
                redact_values.push(key.to_string());
            }
        }
        IntegrationKind::Prometheus => {
            if let (Some(user), Some(pass)) = (ctx.secret("username"), ctx.secret("password")) {
                let enc = B64.encode(format!("{user}:{pass}").as_bytes());
                headers.insert("Authorization".to_string(), format!("Basic {enc}"));
                redact_values.push(enc);
            }
        }
        IntegrationKind::Immich | IntegrationKind::Tandoor | IntegrationKind::Healthchecks => {
            let key = ctx.require_secret("apiKey")?;
            let hdr = if ctx.kind == IntegrationKind::Immich {
                "x-api-key"
            } else if ctx.kind == IntegrationKind::Healthchecks {
                "X-Api-Key"
            } else {
                "Authorization"
            };
            let val = if ctx.kind == IntegrationKind::Tandoor {
                format!("Bearer {key}")
            } else {
                key.to_string()
            };
            headers.insert(hdr.to_string(), val);
            redact_values.push(key.to_string());
        }
        IntegrationKind::Jellyfin => {
            if let Some(key) = ctx.secret("apiKey") {
                headers.insert("X-Emby-Token".to_string(), key.to_string());
                redact_values.push(key.to_string());
            }
        }
        IntegrationKind::Miniflux => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("X-Auth-Token".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::Slskd => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("X-API-KEY".to_string(), key.to_string());
            redact_values.push(key.to_string());
        }
        IntegrationKind::AdGuardHome | IntegrationKind::Nextcloud => {
            let user = ctx.require_secret("username")?;
            let pass = ctx.require_secret("password")?;
            let enc = B64.encode(format!("{user}:{pass}").as_bytes());
            headers.insert("Authorization".to_string(), format!("Basic {enc}"));
            redact_values.push(enc);
        }
        IntegrationKind::PiHole => {
            if let Some(key) = ctx.secret("apiKey") {
                headers.insert("X-FTL-SID".to_string(), key.to_string());
                redact_values.push(key.to_string());
            }
        }
        IntegrationKind::TechnitiumDns => {
            if let Some(key) = ctx.secret("apiKey") {
                let mut query = HashMap::new();
                query.insert("token".to_string(), key.to_string());
                return Ok(ResolvedHttpAuth {
                    query,
                    redact_values: vec![key.to_string()],
                    ..Default::default()
                });
            }
        }
        IntegrationKind::Aria2 => {
            if let Some(key) = ctx.secret("apiKey") {
                return Ok(ResolvedHttpAuth {
                    body: Some(IntegrationHttpBodyAuth::JsonArrayPrefix {
                        name: "params".to_string(),
                        value: format!("token:{key}"),
                    }),
                    redact_values: vec![key.to_string()],
                    ..Default::default()
                });
            }
        }
        _ => {}
    }

    Ok(ResolvedHttpAuth {
        headers,
        query: HashMap::new(),
        body: None,
        redact_values,
    })
}

const REDACTED: &str = "[REDACTED]";
/// Shorter credentials can be ordinary words in response data, so they are only redacted on exact match.
const MIN_EMBEDDED_SECRET_LEN: usize = 12;
/// Characters JavaScript's `encodeURIComponent` leaves unescaped.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Removes integration secrets from upstream responses (port of `redactResponseSecrets`).
#[derive(Debug, Default)]
pub struct SecretRedactor {
    /// Encoded secrets that replace a whole string (or scalar) value.
    exact: HashSet<String>,
    /// Encoded secrets replaced wherever they occur inside a string, longest first.
    embedded: Vec<String>,
}

impl SecretRedactor {
    pub fn new(ctx: &IntegrationContext, auth: &ResolvedHttpAuth) -> Self {
        let mut redactor = Self::default();
        for (kind, value) in &ctx.secrets {
            redactor.add(
                value,
                kind != "username" && value.len() >= MIN_EMBEDDED_SECRET_LEN,
            );
        }
        if let (Some(user), Some(pass)) = (ctx.secret("username"), ctx.secret("password")) {
            redactor.add_derived(&format!("{user}:{pass}"));
        }
        for value in &auth.redact_values {
            redactor.add_derived(value);
        }
        for value in auth.headers.values() {
            redactor.add_derived(value);
            // "Bearer x" / "Basic x" / "Token x": the credential after the scheme.
            if let Some((_, credential)) = value.split_once(' ') {
                redactor.add_derived(credential);
            }
        }
        for (name, value) in &auth.query {
            redactor.add_derived(value);
            let pair = format!(
                "{}={}",
                utf8_percent_encode(name, URI_COMPONENT),
                utf8_percent_encode(value, URI_COMPONENT)
            );
            redactor.add(&pair, true);
        }
        if let Some(
            IntegrationHttpBodyAuth::JsonField { value, .. }
            | IntegrationHttpBodyAuth::JsonArrayPrefix { value, .. },
        ) = &auth.body
        {
            redactor.add_derived(value);
        }
        redactor
            .embedded
            .sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        redactor.embedded.dedup();
        redactor
    }

    /// Redactor for arbitrary secret values (e.g. custom widget HTTP source secrets or the
    /// header values built from them).
    pub fn from_values<'a>(values: impl IntoIterator<Item = &'a str>) -> Self {
        let mut redactor = Self::default();
        for value in values {
            redactor.add_derived(value);
            if let Some((_, credential)) = value.split_once(' ') {
                redactor.add_derived(credential);
            }
        }
        redactor
            .embedded
            .sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        redactor.embedded.dedup();
        redactor
    }

    fn add_derived(&mut self, value: &str) {
        self.add(value, value.len() >= MIN_EMBEDDED_SECRET_LEN);
    }

    fn add(&mut self, value: &str, embedded: bool) {
        if value.is_empty() {
            return;
        }
        let json = serde_json::to_string(value).unwrap_or_default();
        let b64 = B64.encode(value);
        let variants = [
            value.to_string(),
            json.get(1..json.len().saturating_sub(1))
                .unwrap_or(value)
                .to_string(),
            utf8_percent_encode(value, URI_COMPONENT).to_string(),
            b64.trim_end_matches('=').to_string(),
            URL_SAFE_NO_PAD.encode(value),
            b64,
        ];
        for variant in variants {
            if embedded {
                self.embedded.push(variant.clone());
            }
            self.exact.insert(variant);
        }
    }

    pub fn redact_str(&self, value: &str) -> String {
        if self.exact.contains(value) {
            return REDACTED.to_string();
        }
        let mut out = value.to_string();
        for secret in &self.embedded {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        out
    }

    pub fn redact_value(&self, value: Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.redact_str(&s)),
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|v| self.redact_value(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| (self.redact_str(&k), self.redact_value(v)))
                    .collect(),
            ),
            scalar => {
                if self.exact.contains(&scalar.to_string()) {
                    Value::String(REDACTED.to_string())
                } else {
                    scalar
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn redactor(secrets: &[(&str, &str)], auth: ResolvedHttpAuth) -> SecretRedactor {
        let ctx = IntegrationContext {
            id: "id".to_string(),
            name: "name".to_string(),
            url: "http://localhost".to_string(),
            external_url: None,
            kind: IntegrationKind::Sonarr,
            secrets: secrets
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        };
        SecretRedactor::new(&ctx, &auth)
    }

    #[test]
    fn redacts_nested_json() {
        let r = redactor(&[("apiKey", KEY)], ResolvedHttpAuth::default());
        let body = json!({
            "apiKey": KEY,
            "nested": [{ "url": format!("http://x/?apikey={KEY}") }, 5],
            KEY: "key-named",
        });
        assert_eq!(
            r.redact_value(body),
            json!({
                "apiKey": "[REDACTED]",
                "nested": [{ "url": "http://x/?apikey=[REDACTED]" }, 5],
                "[REDACTED]": "key-named",
            })
        );
    }

    #[test]
    fn redacts_plain_text_and_derived_values() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer tok-0123456789".to_string(),
        );
        let mut query = HashMap::new();
        query.insert("token".to_string(), "abc".to_string());
        let r = redactor(
            &[],
            ResolvedHttpAuth {
                headers,
                query,
                ..Default::default()
            },
        );
        let text = format!(
            "auth tok-0123456789 b64 {} q ?token=abc",
            B64.encode("tok-0123456789")
        );
        assert_eq!(
            r.redact_str(&text),
            "auth [REDACTED] b64 [REDACTED] q ?[REDACTED]"
        );
    }

    #[test]
    fn short_values_only_redacted_on_exact_match() {
        let r = redactor(
            &[("username", "admin-user-name"), ("password", "pw")],
            ResolvedHttpAuth::default(),
        );
        assert_eq!(
            r.redact_value(json!(["a pw b", "pw", "hello admin-user-name"])),
            json!(["a pw b", "[REDACTED]", "hello admin-user-name"])
        );
        assert_eq!(r.redact_str("plain text"), "plain text");
    }
}
