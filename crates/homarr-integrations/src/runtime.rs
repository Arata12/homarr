use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use homarr_common::{HomarrError, HomarrResult};
use homarr_core::HttpClientFactory;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::types::IntegrationContext;

/// Hook supplied by `homarr-services` so integrations such as Plex can route authenticated
/// artwork URLs through the Homarr image proxy without introducing a crate cycle.
#[async_trait]
pub trait ImageProxyProvider: Send + Sync {
    async fn proxy_image(
        &self,
        url: &str,
        headers: HashMap<String, String>,
        trusted_origin: Option<String>,
    ) -> HomarrResult<String>;
}

/// Shared state handed to every integration call: the outbound HTTP factory (SSRF rules, trusted
/// certificates, timeouts), optional image proxy, and the session cache for login based integrations.
#[derive(Clone)]
pub struct IntegrationRuntime {
    pub http: HttpClientFactory,
    pub sessions: SessionCache,
    pub image_proxy: Option<Arc<dyn ImageProxyProvider>>,
}

impl IntegrationRuntime {
    pub fn new(http: HttpClientFactory) -> Self {
        Self {
            http,
            sessions: SessionCache::default(),
            image_proxy: None,
        }
    }

    pub fn with_image_proxy(
        http: HttpClientFactory,
        image_proxy: Arc<dyn ImageProxyProvider>,
    ) -> Self {
        Self {
            http,
            sessions: SessionCache::default(),
            image_proxy: Some(image_proxy),
        }
    }

    /// Builds a `rustls::ClientConfig` trusting public web PKI roots plus Homarr's trusted
    /// certificates from `CertificateManager`.
    pub fn ws_tls_config(&self) -> HomarrResult<Arc<rustls::ClientConfig>> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Ok(certificates) = self.http.certs().list() {
            for info in certificates {
                for block in info.content.split("-----BEGIN CERTIFICATE-----").skip(1) {
                    let Some(body) = block.split("-----END CERTIFICATE-----").next() else {
                        continue;
                    };
                    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
                    if let Ok(der) = B64.decode(compact) {
                        let _ = roots.add(rustls::pki_types::CertificateDer::from(der));
                    }
                }
            }
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|err| HomarrError::Internal(format!("TLS config error: {err}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(Arc::new(config))
    }
}

struct SessionEntry {
    /// Serialized integration configuration (id, url, sorted secrets) the session was created for.
    configuration: String,
    value: Value,
    expires_at: Option<Instant>,
}

/// In-memory cache for login tokens, cookies and sids.
///
/// Port of `base/session-store.ts`: one slot per integration (keyed by id and kind), valid only for the
/// configuration (url + secrets) it was stored with, optionally limited by a TTL so a stored session never
/// outlives the credential it holds. A changed configuration or an expired TTL reads as "no session".
#[derive(Clone, Default)]
pub struct SessionCache {
    entries: Arc<Mutex<HashMap<String, SessionEntry>>>,
}

impl SessionCache {
    fn key(ctx: &IntegrationContext) -> String {
        format!("{}:{}", ctx.id, ctx.kind.as_str())
    }

    /// Mirrors `serializeSessionConfiguration`: secrets are sorted so ordering never matters.
    fn configuration(ctx: &IntegrationContext) -> String {
        let mut secrets: Vec<(&String, &String)> = ctx.secrets.iter().collect();
        secrets.sort();
        serde_json::json!({
            "integrationId": ctx.id,
            "url": ctx.url,
            "secrets": secrets,
        })
        .to_string()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionEntry>> {
        // A poisoned lock only means another thread panicked mid-update; the map stays usable.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns the stored session, or `None` when absent, expired, for a different configuration or of another type.
    pub fn get<T: DeserializeOwned>(&self, ctx: &IntegrationContext) -> Option<T> {
        let key = Self::key(ctx);
        let configuration = Self::configuration(ctx);
        let mut entries = self.lock();
        let entry = entries.get(&key)?;
        let expired = entry.expires_at.is_some_and(|at| at <= Instant::now());
        if expired || entry.configuration != configuration {
            entries.remove(&key);
            return None;
        }
        match serde_json::from_value(entry.value.clone()) {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::warn!(integration = %ctx.name, error = %err, "Ignoring unreadable stored session");
                entries.remove(&key);
                None
            }
        }
    }

    /// Stores a session. Pass `ttl` when the credential has a known lifetime.
    pub fn set<T: Serialize>(&self, ctx: &IntegrationContext, value: &T, ttl: Option<Duration>) {
        let value = match serde_json::to_value(value) {
            Ok(value) => value,
            Err(err) => {
                tracing::error!(integration = %ctx.name, error = %err, "Failed to save session");
                return;
            }
        };
        let entry = SessionEntry {
            configuration: Self::configuration(ctx),
            value,
            expires_at: ttl.map(|ttl| Instant::now() + ttl),
        };
        self.lock().insert(Self::key(ctx), entry);
    }

    pub fn clear(&self, ctx: &IntegrationContext) {
        self.lock().remove(&Self::key(ctx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_definitions::IntegrationKind;

    fn ctx(url: &str, password: &str) -> IntegrationContext {
        IntegrationContext {
            id: "i1".into(),
            name: "Test".into(),
            url: url.into(),
            external_url: None,
            kind: IntegrationKind::QBittorrent,
            secrets: HashMap::from([("password".to_string(), password.to_string())]),
        }
    }

    #[test]
    fn stores_and_reads_back_session() {
        let cache = SessionCache::default();
        let c = ctx("http://a", "pw");
        assert_eq!(cache.get::<String>(&c), None);
        cache.set(&c, &"sid".to_string(), None);
        assert_eq!(cache.get::<String>(&c), Some("sid".to_string()));
        cache.clear(&c);
        assert_eq!(cache.get::<String>(&c), None);
    }

    #[test]
    fn ignores_session_after_configuration_change() {
        let cache = SessionCache::default();
        cache.set(&ctx("http://a", "pw"), &"sid".to_string(), None);
        assert_eq!(cache.get::<String>(&ctx("http://a", "other")), None);
    }

    #[test]
    fn expires_after_ttl() {
        let cache = SessionCache::default();
        let c = ctx("http://a", "pw");
        cache.set(&c, &"sid".to_string(), Some(Duration::ZERO));
        assert_eq!(cache.get::<String>(&c), None);
    }
}
