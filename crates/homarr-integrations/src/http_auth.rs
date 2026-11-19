use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::integration::{
    get_integration_def, IntegrationHttpAuthDefinition, IntegrationHttpBodyAuth,
    IntegrationHttpStandardAuth,
};
use homarr_definitions::IntegrationKind;

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
        | IntegrationKind::Jellystat => {
            let key = ctx.require_secret("apiKey")?;
            headers.insert("Authorization".to_string(), format!("Bearer {key}"));
            redact_values.push(key.to_string());
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
