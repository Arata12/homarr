pub mod capabilities;
pub mod clients;
pub mod error;
pub mod factory;
pub mod groups;
pub mod http_auth;
pub mod runtime;
pub mod types;

pub use factory::IntegrationRegistry;
pub use http_auth::{resolve_declared_http_auth, ResolvedHttpAuth};
pub use runtime::{IntegrationRuntime, SessionCache};
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_core::{AppConfig, CertificateManager, HttpClientFactory};
    use homarr_definitions::integration::all_integration_defs;
    use std::collections::HashMap;

    #[tokio::test]
    async fn registry_supports_mock_and_all_integration_kinds() {
        let cfg = AppConfig::for_test();
        let certs = CertificateManager::new(cfg.local_certificate_path.clone());
        let http = HttpClientFactory::new(&certs).unwrap();
        let registry = IntegrationRegistry::new(http);

        let mock_ctx = IntegrationContext {
            id: "mock1".into(),
            name: "Mock".into(),
            url: "http://mock.local".into(),
            external_url: None,
            kind: homarr_definitions::IntegrationKind::Mock,
            secrets: HashMap::new(),
        };

        let res = registry.test_connection(&mock_ctx).await;
        assert!(res.success);

        let dns = registry.get_dns_hole_summary(&mock_ctx).await.unwrap();
        assert!(dns.ads_blocked_today > 0);

        let dl = registry.get_downloads(&mock_ctx).await.unwrap();
        assert!(!dl.items.is_empty());

        let media = registry.get_media_sessions(&mock_ctx).await.unwrap();
        assert!(!media.is_empty());

        let health = registry.get_health_monitoring(&mock_ctx).await.unwrap();
        assert!(health.cpu_percent > 0.0);

        // Verify auth resolution path doesn't panic across all 92 definitions
        for def in all_integration_defs() {
            let mut secrets = HashMap::new();
            if let Some(first_set) = def.secret_kinds.first() {
                for sk in *first_set {
                    secrets.insert(sk.as_str().to_string(), "test-secret".to_string());
                }
            }
            let ctx = IntegrationContext {
                id: "id".into(),
                name: def.name.into(),
                url: "http://127.0.0.1:8080".into(),
                external_url: None,
                kind: def.kind,
                secrets,
            };
            let _ = registry.resolve_http_auth(&ctx);
        }
    }
}
