pub mod csrf;
pub mod mcp;
pub mod openapi;
pub mod routes;
pub mod state;
pub mod ws;

use axum::{
    routing::{delete, get, patch, post, put},
    Router,
};

pub use state::AppState;

use routes::{
    apps, auth, boards, health, integrations, search_icons_media, system, users_groups,
    widgets_services,
};

pub fn api_router(state: AppState) -> Router {
    Router::new()
        // Health & metadata
        .route("/api/health/live", get(health::live))
        .route("/api/health/ready", get(health::ready))
        .route("/api/info", get(health::info))
        .route("/api/updates", get(health::updates))
        .route("/api/analytics", get(health::analytics))
        .route("/api/openapi", get(openapi::openapi_spec))
        // Auth & API keys
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/session", get(auth::session_info))
        .route("/api/auth/register", post(auth::register))
        .route(
            "/api/api-keys",
            get(auth::list_api_keys).post(auth::create_user_api_key),
        )
        .route("/api/api-keys/{id}", delete(auth::delete_user_api_key))
        // Boards
        .route(
            "/api/boards",
            get(boards::list_boards).post(boards::create_board),
        )
        .route("/api/boards/by-name/{name}", get(boards::get_board_by_name))
        .route("/api/boards/import", post(boards::import_board))
        .route(
            "/api/boards/{id}",
            get(boards::get_board)
                .patch(boards::update_board)
                .delete(boards::delete_board),
        )
        .route("/api/boards/{id}/layout", put(boards::save_board_layout))
        .route("/api/boards/{id}/layouts", put(boards::set_board_layouts))
        .route("/api/boards/{id}/items", post(boards::create_board_item))
        .route(
            "/api/boards/{id}/items/{item_id}",
            patch(boards::update_board_item).delete(boards::delete_board_item),
        )
        .route(
            "/api/boards/{id}/sections",
            post(boards::create_board_section),
        )
        .route(
            "/api/boards/{id}/sections/{section_id}",
            delete(boards::delete_board_section),
        )
        .route(
            "/api/boards/{id}/permissions",
            get(boards::get_board_permissions).put(boards::set_board_permissions),
        )
        .route("/api/boards/{id}/duplicate", post(boards::duplicate_board))
        .route("/api/boards/{id}/export", get(boards::export_board))
        // Apps
        .route("/api/apps", get(apps::list_apps).post(apps::create_app))
        .route("/api/apps/ping", post(apps::ping_url))
        .route(
            "/api/apps/{id}",
            get(apps::get_app)
                .patch(apps::update_app)
                .delete(apps::delete_app),
        )
        // Integrations
        .route("/api/integrations/kinds", get(integrations::list_kinds))
        .route(
            "/api/integrations",
            get(integrations::list_integrations).post(integrations::create_integration),
        )
        .route(
            "/api/integrations/{id}",
            get(integrations::get_integration)
                .patch(integrations::update_integration)
                .delete(integrations::delete_integration),
        )
        .route(
            "/api/integrations/{id}/test",
            post(integrations::test_integration),
        )
        .route(
            "/api/integrations/{id}/data",
            get(integrations::get_integration_data),
        )
        .route(
            "/api/integrations/{id}/stats",
            get(integrations::get_integration_stats),
        )
        .route(
            "/api/integrations/{id}/query",
            post(integrations::query_integration),
        )
        .route(
            "/api/integrations/{id}/actions",
            post(integrations::execute_integration_action),
        )
        .route(
            "/api/integrations/{id}/permissions",
            get(integrations::get_integration_permissions)
                .put(integrations::set_integration_permissions),
        )
        // Users, Groups & Invites
        .route(
            "/api/users",
            get(users_groups::list_users).post(users_groups::create_user),
        )
        .route(
            "/api/users/me/preferences",
            patch(users_groups::update_my_preferences),
        )
        .route(
            "/api/users/{id}",
            get(users_groups::get_user)
                .patch(users_groups::update_user)
                .delete(users_groups::delete_user),
        )
        .route(
            "/api/groups",
            get(users_groups::list_groups).post(users_groups::create_group),
        )
        .route(
            "/api/groups/{id}",
            get(users_groups::get_group)
                .patch(users_groups::update_group)
                .delete(users_groups::delete_group),
        )
        .route(
            "/api/groups/{id}/permissions",
            put(users_groups::set_group_permissions),
        )
        .route(
            "/api/groups/{id}/members/{user_id}",
            post(users_groups::add_group_member).delete(users_groups::remove_group_member),
        )
        .route(
            "/api/invites",
            get(users_groups::list_invites).post(users_groups::create_invite),
        )
        .route("/api/invites/{id}", delete(users_groups::delete_invite))
        // Search engines, icons, media & image proxy
        .route(
            "/api/search-engines",
            get(search_icons_media::list_search_engines)
                .post(search_icons_media::create_search_engine),
        )
        .route(
            "/api/search-engines/{id}",
            patch(search_icons_media::update_search_engine)
                .delete(search_icons_media::delete_search_engine),
        )
        .route("/api/search/ddg-bangs", get(search_icons_media::ddg_bangs))
        .route("/api/icons", get(search_icons_media::search_icons))
        .route("/api/icons/sync", post(search_icons_media::sync_icons))
        .route("/api/icons/favicon", get(search_icons_media::fetch_favicon))
        .route(
            "/api/medias",
            get(search_icons_media::list_medias).post(search_icons_media::upload_media),
        )
        .route(
            "/api/medias/{id}",
            get(search_icons_media::get_media).delete(search_icons_media::delete_media),
        )
        .route(
            "/api/image-proxy/forward",
            post(search_icons_media::image_proxy_forward),
        )
        .route(
            "/api/image-proxy/{id}",
            get(search_icons_media::image_proxy_get),
        )
        // Widgets, Custom Widgets, Docker, Kubernetes & External telemetry
        .route(
            "/api/widgets/catalog",
            get(widgets_services::widget_catalog),
        )
        .route(
            "/api/widgets/render/{kind}",
            get(widgets_services::render_widget),
        )
        .route(
            "/api/widgets/resolve",
            post(widgets_services::resolve_widget),
        )
        .route(
            "/api/custom-widgets",
            get(widgets_services::list_custom_widgets).post(widgets_services::create_custom_widget),
        )
        .route(
            "/api/custom-widgets/{id}",
            get(widgets_services::get_custom_widget)
                .patch(widgets_services::update_custom_widget)
                .delete(widgets_services::delete_custom_widget),
        )
        .route(
            "/api/custom-widgets/{id}/execute",
            post(widgets_services::execute_custom_widget),
        )
        .route(
            "/api/workshop/listings",
            get(widgets_services::workshop_listings),
        )
        .route(
            "/api/workshop/install",
            post(widgets_services::workshop_install),
        )
        .route(
            "/api/docker/containers",
            get(widgets_services::docker_containers),
        )
        .route(
            "/api/docker/containers/{id}/{action}",
            post(widgets_services::docker_container_action),
        )
        .route(
            "/api/docker/containers/{id}/logs",
            get(widgets_services::docker_container_logs),
        )
        .route(
            "/api/docker/discover",
            get(widgets_services::docker_discover),
        )
        .route(
            "/api/kubernetes/{resource}",
            get(widgets_services::kubernetes_resource),
        )
        .route(
            "/api/external/weather",
            get(widgets_services::external_weather),
        )
        .route(
            "/api/external/air-quality",
            get(widgets_services::external_air_quality),
        )
        .route(
            "/api/external/cities",
            get(widgets_services::external_cities),
        )
        .route("/api/external/rss", get(widgets_services::external_rss))
        .route(
            "/api/external/stocks",
            get(widgets_services::external_stocks),
        )
        .route(
            "/api/external/releases",
            get(widgets_services::external_releases),
        )
        .route(
            "/api/external/minecraft",
            get(widgets_services::external_minecraft),
        )
        // System, Settings, Tasks, Certs, Logs, Backup, Assistant
        .route(
            "/api/settings",
            get(system::get_settings).patch(system::patch_settings),
        )
        .route(
            "/api/onboarding",
            get(system::get_onboarding).post(system::update_onboarding),
        )
        .route("/api/cron-jobs", get(system::list_cron_jobs))
        .route(
            "/api/cron-jobs/{name}/trigger",
            post(system::trigger_cron_job),
        )
        .route("/api/cron-jobs/{name}", patch(system::update_cron_job))
        .route(
            "/api/certificates",
            get(system::list_certificates).post(system::upload_certificate),
        )
        .route(
            "/api/certificates/{name}",
            delete(system::delete_certificate),
        )
        .route("/api/logs", get(system::recent_logs))
        .route("/api/backup/export", get(system::export_backup))
        .route("/api/backup/import", post(system::import_backup))
        .route(
            "/api/assistant/config",
            get(system::get_assistant_config).put(system::save_assistant_config),
        )
        .route(
            "/api/assistant/threads",
            get(system::list_assistant_threads).post(system::create_assistant_thread),
        )
        .route(
            "/api/assistant/threads/{id}",
            delete(system::delete_assistant_thread),
        )
        .route(
            "/api/assistant/threads/{id}/messages",
            get(system::list_assistant_messages).post(system::send_assistant_message),
        )
        // MCP JSON-RPC 2.0 + OAuth discovery
        .route("/api/mcp", post(mcp::mcp_endpoint))
        .route(
            "/.well-known/oauth-protected-resource",
            get(mcp::oauth_protected_resource),
        )
        .route("/api/mcp/oauth/register", post(mcp::oauth_register))
        .route("/api/mcp/oauth/authorize", get(mcp::oauth_authorize))
        .route("/api/mcp/oauth/token", post(mcp::oauth_token))
        // WebSockets (explicitly reject ApiKey header before WebSocketUpgrade)
        .route(
            "/websockets",
            get(ws::ws_handler)
                .route_layer(axum::middleware::from_fn(ws::reject_api_key_middleware)),
        )
        .route(
            "/ws",
            get(ws::ws_handler)
                .route_layer(axum::middleware::from_fn(ws::reject_api_key_middleware)),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf::csrf_middleware,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use homarr_auth::create_api_key;
    use homarr_common::SecretCrypto;
    use homarr_core::{AppConfig, CertificateManager, DatabaseDriver, HttpClientFactory, LogStore};
    use homarr_db::{repos, seed, Database};
    use homarr_redis::RedisStore;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn setup_test_state() -> (AppState, Router) {
        setup_test_state_with(|_| {}).await
    }

    async fn setup_test_state_with(configure: impl FnOnce(&mut AppConfig)) -> (AppState, Router) {
        let mut cfg = AppConfig::for_test();
        cfg.demo_mode = true;
        cfg.demo_read_only = false;
        configure(&mut cfg);
        let certs = CertificateManager::new(cfg.local_certificate_path.clone());
        let http = HttpClientFactory::new(&certs).unwrap();
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let crypto = SecretCrypto::zero_for_test();
        seed::seed_demo(&db, &crypto).await.unwrap();
        let redis = RedisStore::in_memory();
        let logs = LogStore::new();
        let state = AppState::build(cfg, db, redis, crypto, logs, certs, http);
        let app = api_router(state.clone());
        (state, app)
    }

    #[tokio::test]
    async fn api_health_auth_crud_openapi_mcp_and_ws_guards_work() {
        let (state, app) = setup_test_state().await;

        // 1. Live & ready health checks
        let resp = app
            .clone()
            .oneshot(
                Request::get("/api/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .clone()
            .oneshot(
                Request::get("/api/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. OpenAPI 3.1 spec
        let resp = app
            .clone()
            .oneshot(Request::get("/api/openapi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let spec: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["paths"]["/api/mcp"].is_object());

        // 3. Login with demo/demo sets session cookie
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"demo","password":"demo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        // 4. Cookie-authenticated writes need a same-origin Origin (CSRF)
        let csrf_resp = app
            .clone()
            .oneshot(
                Request::post("/api/apps")
                    .header(header::HOST, "localhost")
                    .header(header::ORIGIN, "http://evil.example")
                    .header(header::COOKIE, &set_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"X"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(csrf_resp.status(), StatusCode::FORBIDDEN);

        // Create -> list -> delete App with cookie
        let create_resp = app
            .clone()
            .oneshot(
                Request::post("/api/apps")
                    .header(header::HOST, "localhost")
                    .header(header::ORIGIN, "http://localhost")
                    .header(header::COOKIE, &set_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Grafana","iconUrl":"/logo.png","href":"https://grafana.local"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created_bytes = to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
        let created_app: Value = serde_json::from_slice(&created_bytes).unwrap();
        let app_id = created_app["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(
                Request::delete(format!("/api/apps/{app_id}"))
                    .header(header::HOST, "localhost")
                    .header(header::ORIGIN, "http://localhost")
                    .header(header::COOKIE, &set_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::OK);

        // 5. MCP tools/list + read-only API key blocks write tool
        let demo_user = repos::users::find_by_name_ci(&state.db, "demo")
            .await
            .unwrap()
            .unwrap();
        let (ro_key, _) = create_api_key(&state.db, &demo_user.id, true, None)
            .await
            .unwrap();

        let mcp_list = app
            .clone()
            .oneshot(
                Request::post("/api/mcp")
                    .header("ApiKey", &ro_key)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mcp_list.status(), StatusCode::OK);
        let list_json: Value =
            serde_json::from_slice(&to_bytes(mcp_list.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert!(!list_json["result"]["tools"].as_array().unwrap().is_empty());

        let mcp_write_denied = app
            .clone()
            .oneshot(
                Request::post("/api/mcp")
                    .header("ApiKey", &ro_key)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 2,
                            "method": "tools/call",
                            "params": {
                                "name": "homarr_create_app",
                                "arguments": { "name": "Blocked", "iconUrl": "/logo.png" }
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_json: Value = serde_json::from_slice(
            &to_bytes(mcp_write_denied.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(denied_json["error"]["code"], -32003);

        // 6. WebSocket rejects ApiKey header
        let ws_resp = app
            .oneshot(
                Request::get("/websockets")
                    .header("ApiKey", &ro_key)
                    .header(header::CONNECTION, "Upgrade")
                    .header(header::UPGRADE, "websocket")
                    .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
                    .header("Sec-WebSocket-Version", "13")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn onboarding_update_requires_admin_once_a_user_exists() {
        let (_state, app) = setup_test_state().await;
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/onboarding")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"step":"finish"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn trusted_proxy_header_requires_allowed_peer_address() {
        let (_state, app) = setup_test_state_with(|cfg| {
            cfg.auth_trusted_proxy_header = Some("Remote-User".to_string());
            cfg.auth_trusted_proxy_ips = vec!["10.0.0.0/8".to_string()];
        })
        .await;

        async fn authenticated(app: &Router, peer: Option<&str>) -> bool {
            let mut req = Request::get("/api/auth/session")
                .header("Remote-User", "demo")
                .body(Body::empty())
                .unwrap();
            if let Some(peer) = peer {
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        peer.parse().unwrap(),
                    ));
            }
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            body["authenticated"].as_bool().unwrap()
        }

        assert!(!authenticated(&app, None).await);
        assert!(!authenticated(&app, Some("203.0.113.9:4000")).await);
        assert!(authenticated(&app, Some("10.1.2.3:4000")).await);
    }
}
