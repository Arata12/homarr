use std::sync::Arc;

use homarr_auth::AuthContextProvider;
use homarr_common::SecretCrypto;
use homarr_core::{AppConfig, CertificateManager, HttpClientFactory, LogStore};
use homarr_db::Database;
use homarr_integrations::IntegrationRegistry;
use homarr_redis::RedisStore;
use homarr_services::ServiceContainer;
use homarr_tasks::TaskScheduler;
use homarr_widgets::WidgetResolver;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: Database,
    pub redis: RedisStore,
    pub crypto: SecretCrypto,
    pub logs: LogStore,
    pub certs: CertificateManager,
    pub http: HttpClientFactory,
    pub integrations: IntegrationRegistry,
    pub services: ServiceContainer,
    pub scheduler: TaskScheduler,
    pub widgets: WidgetResolver,
}

impl AppState {
    pub fn build(
        config: AppConfig,
        db: Database,
        redis: RedisStore,
        crypto: SecretCrypto,
        logs: LogStore,
        certs: CertificateManager,
        http: HttpClientFactory,
    ) -> Self {
        let integrations = IntegrationRegistry::new(http.clone());
        let services =
            ServiceContainer::new(config.clone(), crypto.clone(), http.clone(), redis.clone());
        let scheduler = TaskScheduler::new(
            config.clone(),
            db.clone(),
            redis.clone(),
            crypto.clone(),
            integrations.clone(),
            services.clone(),
        );
        let widgets = WidgetResolver::new(
            db.clone(),
            crypto.clone(),
            integrations.clone(),
            services.clone(),
        );
        Self {
            config: Arc::new(config),
            db,
            redis,
            crypto,
            logs,
            certs,
            http,
            integrations,
            services,
            scheduler,
            widgets,
        }
    }

    pub fn demo_read_only_active(&self) -> bool {
        self.config.demo_mode && self.config.demo_read_only
    }
}

impl AuthContextProvider for AppState {
    fn db(&self) -> &Database {
        &self.db
    }

    fn config(&self) -> &AppConfig {
        &self.config
    }
}
