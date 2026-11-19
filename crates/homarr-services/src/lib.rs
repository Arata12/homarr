pub mod assistant;
pub mod custom_widgets;
pub mod docker;
pub mod external;
pub mod icons;
pub mod image_proxy;
pub mod kubernetes;
pub mod ping;

use std::sync::Arc;

use homarr_common::SecretCrypto;
use homarr_core::{AppConfig, HttpClientFactory};
use homarr_redis::RedisStore;

pub use assistant::AssistantService;
pub use custom_widgets::{
    render_template, validate_custom_widget_definition, CustomWidgetService, WorkshopListing,
};
pub use docker::{decode_docker_logs, DiscoveredDockerService, DockerContainerInfo, DockerService};
pub use external::{
    AirQualityData, AnalyticsSnapshot, DuckDuckGoBang, ExternalDataService, FeedEntry,
    GeocodingCity, MinecraftStatus, ReleaseInfo, StockPriceInfo, UpdateCheckSummary, WeatherData,
};
pub use icons::{IconService, IconSyncSummary};
pub use image_proxy::{ImageProxyResult, ImageProxyService};
pub use kubernetes::KubernetesService;
pub use ping::{PingResult, PingService};

#[derive(Clone)]
pub struct ServiceContainer {
    pub ping: Arc<PingService>,
    pub image_proxy: Arc<ImageProxyService>,
    pub icons: Arc<IconService>,
    pub docker: Arc<DockerService>,
    pub kubernetes: Arc<KubernetesService>,
    pub custom_widgets: Arc<CustomWidgetService>,
    pub external: Arc<ExternalDataService>,
    pub assistant: Arc<AssistantService>,
}

impl ServiceContainer {
    pub fn new(
        config: AppConfig,
        crypto: SecretCrypto,
        http: HttpClientFactory,
        redis: RedisStore,
    ) -> Self {
        Self {
            ping: Arc::new(PingService::new(http.clone(), redis.clone())),
            image_proxy: Arc::new(ImageProxyService::new(&crypto, http.clone(), redis.clone())),
            icons: Arc::new(IconService::new(config.clone(), http.clone())),
            docker: Arc::new(DockerService::new(config.clone())),
            kubernetes: Arc::new(KubernetesService::new(config.clone(), http.clone())),
            custom_widgets: Arc::new(CustomWidgetService::new(config.clone(), http.clone())),
            external: Arc::new(ExternalDataService::new(
                config.clone(),
                http.clone(),
                redis,
            )),
            assistant: Arc::new(AssistantService::new(config, crypto, http)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_core::CertificateManager;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn image_proxy_persistent_sign_and_tamper_rejection() {
        let cfg = AppConfig::for_test();
        let certs = CertificateManager::new(cfg.local_certificate_path);
        let http = HttpClientFactory::new(&certs).unwrap();
        let crypto = SecretCrypto::zero_for_test();
        let redis = RedisStore::in_memory();
        let proxy = ImageProxyService::new(&crypto, http, redis);

        assert_ne!(proxy.derived_key(), crypto.raw_key());

        let url = proxy
            .create_persistent_image_url(
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/sonarr.svg",
            )
            .unwrap();
        let id = url.strip_prefix("/api/image-proxy/").unwrap();
        let verified = proxy.verify_persistent_token(id).unwrap();
        assert_eq!(
            verified.as_deref(),
            Some("https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/sonarr.svg")
        );

        let mut tampered = id.to_string();
        let last = if tampered.ends_with('a') { 'b' } else { 'a' };
        tampered.pop();
        tampered.push(last);
        assert!(proxy.verify_persistent_token(&tampered).is_err());
    }

    #[test]
    fn custom_widget_security_validation_and_template_escape() {
        let valid = json!({
            "schema": "homarr-custom-widget-v2",
            "name": "Status Card",
            "template": "<div>{{title}}</div>",
            "sources": [],
            "requests": []
        });
        assert!(validate_custom_widget_definition(&valid).is_ok());

        let with_secret = json!({
            "schema": "homarr-custom-widget-v2",
            "name": "Bad",
            "template": "<div>{{ok}}</div>",
            "sources": [{ "id": "s1", "token": "sk-1234567890abcdefghij" }]
        });
        assert!(validate_custom_widget_definition(&with_secret).is_err());

        let with_proto = json!({
            "schema": "homarr-custom-widget-v2",
            "name": "BadProto",
            "template": "<div>ok</div>",
            "__proto__": { "polluted": true }
        });
        assert!(validate_custom_widget_definition(&with_proto).is_err());

        let with_xss = json!({
            "schema": "homarr-custom-widget-v2",
            "name": "BadXss",
            "template": "<script>alert(1)</script>"
        });
        assert!(validate_custom_widget_definition(&with_xss).is_err());

        let rendered = render_template(
            "<span>{{msg}}</span>",
            &json!({ "msg": "<script>xss</script>" }),
            &HashMap::new(),
        );
        assert!(!rendered.contains("<script>"));
        assert!(rendered.contains("&lt;script&gt;"));
    }

    #[test]
    fn parses_rss_and_atom_xml() {
        let rss = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0">
          <channel>
            <title>Homarr Blog</title>
            <item>
              <title>Release v2.13</title>
              <link>https://homarr.dev/blog/v2-13</link>
              <pubDate>Tue, 18 Nov 2026 12:00:00 GMT</pubDate>
              <description>Major update</description>
            </item>
          </channel>
        </rss>"#;
        let rss_entries = external::parse_rss_or_atom(rss);
        assert_eq!(rss_entries.len(), 1);
        assert_eq!(rss_entries[0].title, "Release v2.13");
        assert_eq!(rss_entries[0].link, "https://homarr.dev/blog/v2-13");

        let atom = r#"<?xml version="1.0" encoding="utf-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <title>GitHub Releases</title>
          <entry>
            <title>v2.13.1</title>
            <link href="https://github.com/homarr-labs/homarr/releases/tag/v2.13.1" />
            <updated>2026-11-18T12:00:00Z</updated>
          </entry>
        </feed>"#;
        let atom_entries = external::parse_rss_or_atom(atom);
        assert_eq!(atom_entries.len(), 1);
        assert_eq!(atom_entries[0].title, "v2.13.1");
        assert_eq!(
            atom_entries[0].link,
            "https://github.com/homarr-labs/homarr/releases/tag/v2.13.1"
        );
    }

    #[test]
    fn decodes_docker_multiplexed_and_plain_logs() {
        let payload = b"hello docker\n";
        let mut framed = vec![1, 0, 0, 0];
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(payload);
        assert_eq!(decode_docker_logs(&framed), "hello docker\n");
        assert_eq!(decode_docker_logs(b"plain text log"), "plain text log");
    }
}
