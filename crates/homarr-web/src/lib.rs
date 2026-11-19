pub mod i18n;
pub mod icons;
pub mod layout;
pub mod pages;

use axum::{routing::get, Router};
use homarr_api::AppState;
use homarr_auth::AuthContextProvider;
use homarr_core::AppConfig;
use homarr_db::Database;

use i18n::I18nCatalog;
use pages::{auth, board, manage};

#[derive(Clone)]
pub struct WebState {
    pub app: AppState,
    pub i18n: I18nCatalog,
}

impl AuthContextProvider for WebState {
    fn db(&self) -> &Database {
        &self.app.db
    }

    fn config(&self) -> &AppConfig {
        &self.app.config
    }
}

pub fn web_router(state: AppState) -> Router {
    let i18n = I18nCatalog::load(&state.config.assets_dir.join("locales"));
    let web_state = WebState { app: state, i18n };

    Router::new()
        .route("/", get(board::home_page))
        .route("/boards", get(board::boards_index_page))
        .route("/boards/{name}", get(board::named_board_page))
        .route("/boards/{name}/settings", get(board::board_settings_page))
        .route("/auth/login", get(auth::login_page))
        .route("/auth/invite/{id}", get(auth::invite_page))
        .route("/auth/mcp", get(auth::mcp_auth_page))
        .route("/init", get(auth::onboarding_page))
        .route("/manage", get(manage::overview))
        .route("/manage/boards", get(manage::boards_page))
        .route("/manage/apps", get(manage::apps_page))
        .route("/manage/integrations", get(manage::integrations_page))
        .route("/manage/search-engines", get(manage::search_engines_page))
        .route("/manage/users", get(manage::users_page))
        .route("/manage/users/groups", get(manage::groups_page))
        .route("/manage/users/invites", get(manage::invites_page))
        .route("/manage/custom-widgets", get(manage::custom_widgets_page))
        .route("/manage/settings", get(manage::settings_page))
        .route("/manage/tools/docker", get(manage::docker_page))
        .route("/manage/tools/kubernetes", get(manage::kubernetes_page))
        .route("/manage/tools/tasks", get(manage::tasks_page))
        .route("/manage/tools/logs", get(manage::logs_page))
        .route("/manage/tools/certificates", get(manage::certificates_page))
        .route("/manage/tools/api-keys", get(manage::api_keys_page))
        .route("/manage/tools/backup", get(manage::backup_page))
        .route("/manage/assistant", get(manage::assistant_page))
        .route("/manage/about", get(manage::about_page))
        .route("/reference/api", get(manage::api_reference_page))
        .route("/widgets/{kind}", get(manage::widget_preview_page))
        .with_state(web_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use homarr_common::SecretCrypto;
    use homarr_core::{AppConfig, CertificateManager, DatabaseDriver, HttpClientFactory, LogStore};
    use homarr_db::{seed, Database};
    use homarr_redis::RedisStore;
    use std::path::PathBuf;
    use tower::ServiceExt;

    #[test]
    fn i18n_loads_36_locales() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
        let cat = i18n::I18nCatalog::load(&dir);
        assert_eq!(cat.len(), 36);
        assert_eq!(
            cat.t("en", "init.studio.welcome.title", "fallback"),
            "Welcome home."
        );
    }

    #[test]
    fn i18n_loads_locales_lazily() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
        let cat = i18n::I18nCatalog::load(&dir);
        assert_eq!(cat.len(), 36);
        assert_eq!(cat.cached_count(), 0);
        let _ = cat.t("en", "init.studio.welcome.title", "fallback");
        assert_eq!(cat.cached_count(), 1);
    }

    #[tokio::test]
    async fn web_routes_render_ssr_html() {
        let mut cfg = AppConfig::for_test();
        cfg.demo_mode = true;
        cfg.assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let certs = CertificateManager::new(cfg.local_certificate_path.clone());
        let http = HttpClientFactory::new(&certs).unwrap();
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let crypto = SecretCrypto::zero_for_test();
        seed::seed_demo(&db, &crypto).await.unwrap();
        let state = AppState::build(
            cfg,
            db,
            RedisStore::in_memory(),
            crypto,
            LogStore::new(),
            certs,
            http,
        );

        let app = web_router(state);
        for path in [
            "/",
            "/auth/login",
            "/init",
            "/reference/api",
            "/widgets/clock",
        ] {
            let resp = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "path {path} failed");
            let html = String::from_utf8(
                to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.contains("Homarr"), "path {path} missing Homarr markup");
        }
    }
}
