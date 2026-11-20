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
            "/api/boards/{id}/sections/{section_id}/collapse",
            put(boards::set_section_collapsed),
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
        // Upstream Homarr stores uploaded media URLs as /api/user-medias/{id}.
        .route("/api/user-medias/{id}", get(search_icons_media::get_media))
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
        .route(
            "/api/mcp/oauth/authorize",
            get(mcp::oauth_authorize).post(mcp::oauth_authorize_decision),
        )
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

    async fn login_cookie(app: &Router, name: &str, password: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "name": name, "password": password }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn send_json(
        app: &Router,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "localhost")
            .header(header::ORIGIN, "http://localhost")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        let resp = app
            .clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn create_regular_user(app: &Router, admin: &str, name: &str, password: &str) -> String {
        let (status, created) = send_json(
            app,
            "POST",
            "/api/users",
            Some(admin),
            json!({ "name": name, "password": password }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "create user: {created}");
        created["user"]["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn image_proxy_forward_requires_admin() {
        let (_state, app) = setup_test_state().await;
        let body = json!({ "url": "https://example.com/x.html" });
        let (status, _) =
            send_json(&app, "POST", "/api/image-proxy/forward", None, body.clone()).await;
        assert!(matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));

        let admin = login_cookie(&app, "demo", "demo").await;
        create_regular_user(&app, &admin, "viewer", "viewerpass1").await;
        let viewer = login_cookie(&app, "viewer", "viewerpass1").await;
        let (status, _) = send_json(
            &app,
            "POST",
            "/api/image-proxy/forward",
            Some(&viewer),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn app_ping_rejects_raw_urls_and_unknown_apps() {
        let (_state, app) = setup_test_state().await;
        let (status, _) = send_json(
            &app,
            "POST",
            "/api/apps/ping",
            None,
            json!({ "url": "http://127.0.0.1" }),
        )
        .await;
        assert!(status.is_client_error(), "got {status}");
        let (status, _) = send_json(
            &app,
            "POST",
            "/api/apps/ping",
            None,
            json!({ "appId": "does-not-exist" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn api_keys_require_admin() {
        let (_state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        create_regular_user(&app, &admin, "alice", "alicepass1").await;
        let alice = login_cookie(&app, "alice", "alicepass1").await;
        let (status, _) = send_json(&app, "POST", "/api/api-keys", Some(&alice), json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send_json(&app, "POST", "/api/api-keys", Some(&admin), json!({})).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn changing_own_password_requires_previous_password() {
        let (_state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let bob_id = create_regular_user(&app, &admin, "bob", "bobpass12").await;
        let uri = format!("/api/users/{bob_id}");
        let bob = login_cookie(&app, "bob", "bobpass12").await;

        let (status, _) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&bob),
            json!({ "password": "newpass123" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&bob),
            json!({ "password": "newpass123", "previousPassword": "wrongpass" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&bob),
            json!({ "password": "newpass123", "previousPassword": "bobpass12" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        login_cookie(&app, "bob", "newpass123").await;

        // Admins resetting someone else's password do not need the previous one.
        let (status, _) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&admin),
            json!({ "password": "adminset1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn modify_permission_cannot_publish_board_or_add_custom_widget() {
        let (state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let user_id = create_regular_user(&app, &admin, "modifier", "modifier1").await;
        let user = login_cookie(&app, "modifier", "modifier1").await;
        let board = repos::boards::create_board_with_defaults(
            &state.db,
            repos::boards::CreateBoardInput {
                id: None,
                name: "private-board".into(),
                is_public: false,
                creator_id: None,
                column_count: 10,
            },
        )
        .await
        .unwrap();
        let board_id = board.board.id.clone();

        let (status, body) = send_json(
            &app,
            "PUT",
            &format!("/api/boards/{board_id}/permissions"),
            Some(&admin),
            json!({ "users": [{ "userId": user_id, "permission": "modify" }] }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "set permissions: {body}");

        let (status, _) = send_json(
            &app,
            "PATCH",
            &format!("/api/boards/{board_id}"),
            Some(&user),
            json!({ "isPublic": true }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let stored = repos::boards::get_by_id(&state.db, &board_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!stored.is_public);

        let custom_item = json!({
            "id": "",
            "kind": "customApi",
            "options": { "customWidgetId": "cw" },
            "layouts": [{
                "itemId": "",
                "sectionId": board.sections[0].id,
                "layoutId": board.layouts[0].id,
                "xOffset": 0, "yOffset": 0, "width": 1, "height": 1
            }]
        });
        let (status, _) = send_json(
            &app,
            "POST",
            &format!("/api/boards/{board_id}/items"),
            Some(&user),
            custom_item,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    async fn mcp_approve(app: &Router, cookie: &str, client_id: &str, challenge: &str) -> String {
        let uri = format!(
            "/api/mcp/oauth/authorize?response_type=code&client_id={client_id}\
             &redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcb&code_challenge={challenge}\
             &code_challenge_method=S256&state=a%26b%3Dc"
        );
        let resp = app
            .clone()
            .oneshot(
                Request::get(uri)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let html = String::from_utf8(
            to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let pending = html
            .split(r#"name="pending" value=""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/mcp/oauth/authorize")
                    .header(header::COOKIE, cookie)
                    .header(header::HOST, "localhost")
                    .header(header::ORIGIN, "http://localhost")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("pending={pending}&decision=approve")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let location = url::Url::parse(resp.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        assert!(location.as_str().starts_with("http://127.0.0.1:9999/cb?"));
        let pairs: std::collections::HashMap<String, String> =
            location.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("state").map(String::as_str), Some("a&b=c"));
        pairs["code"].clone()
    }

    #[tokio::test]
    async fn mcp_oauth_requires_registered_client_consent_post_and_pkce() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use sha2::{Digest, Sha256};

        let (state, app) = setup_test_state().await;
        let cookie = login_cookie(&app, "demo", "demo").await;

        // Old attack: GET with approve=1 for an unregistered client must not redirect with a code.
        let resp = app
            .clone()
            .oneshot(
                Request::get(
                    "/api/mcp/oauth/authorize?client_id=evil\
                     &redirect_uri=https%3A%2F%2Fevil.example%2Fcb&approve=1&state=x",
                )
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(!resp.status().is_redirection());
        assert!(resp.headers().get(header::LOCATION).is_none());

        let (status, reg) = send_json(
            &app,
            "POST",
            "/api/mcp/oauth/register",
            None,
            json!({ "client_name": "Test", "redirect_uris": ["http://127.0.0.1:9999/cb"] }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "register: {reg}");
        let client_id = reg["client_id"].as_str().unwrap().to_string();

        // Registered client but unregistered redirect_uri: no redirect.
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/mcp/oauth/authorize?response_type=code&client_id={client_id}\
                     &redirect_uri=https%3A%2F%2Fevil.example%2Fcb&code_challenge={}\
                     &code_challenge_method=S256",
                    "A".repeat(43)
                ))
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Decision POST without Origin is rejected.
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/mcp/oauth/authorize")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("pending=x&decision=approve"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let verifier = "v".repeat(43);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let token_req = |code: &str, verifier: &str| {
            Request::post("/api/mcp/oauth/token")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "grant_type=authorization_code&code={code}&client_id={client_id}\
                     &redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcb&code_verifier={verifier}"
                )))
                .unwrap()
        };

        // Happy path: correct verifier yields a read-only key; the code is single-use.
        let code = mcp_approve(&app, &cookie, &client_id, &challenge).await;
        let resp = app
            .clone()
            .oneshot(token_req(&code, &verifier))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let tok: Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(tok["scope"], "mcp:tools:read");
        let sess =
            homarr_auth::authenticate_api_key(&state.db, tok["access_token"].as_str().unwrap())
                .await
                .unwrap()
                .unwrap();
        assert!(sess.api_key_read_only);
        let resp = app
            .clone()
            .oneshot(token_req(&code, &verifier))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Wrong verifier fails.
        let code = mcp_approve(&app, &cookie, &client_id, &challenge).await;
        let resp = app
            .clone()
            .oneshot(token_req(&code, &"w".repeat(43)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_apps_rejects_anonymous_and_regular_users() {
        let (_state, app) = setup_test_state().await;
        let (status, _) = send_json(&app, "GET", "/api/apps", None, json!({})).await;
        assert!(matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));

        let admin = login_cookie(&app, "demo", "demo").await;
        create_regular_user(&app, &admin, "appviewer", "appviewer1").await;
        let viewer = login_cookie(&app, "appviewer", "appviewer1").await;
        let (status, _) = send_json(&app, "GET", "/api/apps", Some(&viewer), json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = send_json(&app, "GET", "/api/apps", Some(&admin), json!({})).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn get_app_hides_apps_not_on_viewable_boards() {
        let (_state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let (status, created) = send_json(
            &app,
            "POST",
            "/api/apps",
            Some(&admin),
            json!({ "name": "Hidden", "iconUrl": "/logo.png", "href": "http://10.0.0.5" }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let uri = format!("/api/apps/{}", created["id"].as_str().unwrap());

        let (status, _) = send_json(&app, "GET", &uri, None, json!({})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        create_regular_user(&app, &admin, "appviewer2", "appviewer2").await;
        let viewer = login_cookie(&app, "appviewer2", "appviewer2").await;
        let (status, _) = send_json(&app, "GET", &uri, Some(&viewer), json!({})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send_json(&app, "GET", &uri, Some(&admin), json!({})).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn renaming_user_onto_existing_name_conflicts() {
        let (_state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        create_regular_user(&app, &admin, "alice", "password123").await;
        let bob = create_regular_user(&app, &admin, "bob", "password123").await;
        let uri = format!("/api/users/{bob}");
        let (status, body) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&admin),
            json!({ "name": "ALICE" }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        let (status, body) =
            send_json(&app, "PATCH", &uri, Some(&admin), json!({ "name": "Bob" })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    #[tokio::test]
    async fn everyone_group_is_protected() {
        let (state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let everyone = repos::groups::get_by_name(&state.db, "everyone")
            .await
            .unwrap()
            .expect("everyone group seeded");
        let uri = format!("/api/groups/{}", everyone.id);

        let (status, _) = send_json(&app, "DELETE", &uri, Some(&admin), json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send_json(
            &app,
            "PATCH",
            &uri,
            Some(&admin),
            json!({ "name": "renamed" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send_json(
            &app,
            "POST",
            "/api/groups",
            Some(&admin),
            json!({ "name": "Everyone" }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn anonymous_external_fetches_and_media_list_are_rejected() {
        let (_state, app) = setup_test_state().await;
        let (status, _) = send_json(
            &app,
            "GET",
            "/api/external/rss?url=http%3A%2F%2F127.0.0.1%3A1%2Ffeed.xml",
            None,
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send_json(
            &app,
            "GET",
            "/api/icons/favicon?url=http%3A%2F%2F127.0.0.1%3A1%2F",
            None,
            json!({}),
        )
        .await;
        assert!(matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));
        let (status, _) = send_json(&app, "GET", "/api/medias", None, json!({})).await;
        assert!(matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ));
        let (status, _) = send_json(
            &app,
            "POST",
            "/api/widgets/resolve",
            None,
            json!({ "kind": "rssFeed", "options": { "feedUrls": ["http://127.0.0.1:1/x"] } }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn assistant_config_rejects_unsafe_urls_and_drops_key_on_new_destination() {
        let (_state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let put = |body: Value| {
            let admin = admin.clone();
            let app = app.clone();
            async move { send_json(&app, "PUT", "/api/assistant/config", Some(&admin), body).await }
        };

        for bad in [
            "https://user:pass@example.com/v1",
            "https://example.com/v1?key=1",
            "https://example.com/v1#frag",
            "ftp://example.com/v1",
        ] {
            let (status, _) =
                put(json!({ "enabled": false, "provider": "openai", "baseUrl": bad })).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} should be rejected");
        }

        let (status, body) = put(json!({
            "enabled": false,
            "provider": "openai",
            "baseUrl": "https://a.example.com/v1",
            "apiKey": "sk-test"
        }))
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");

        put(json!({ "enabled": false, "provider": "openai", "baseUrl": "https://a.example.com/v1/" }))
            .await;
        let (_, cfg) = send_json(
            &app,
            "GET",
            "/api/assistant/config",
            Some(&admin),
            json!({}),
        )
        .await;
        assert_eq!(cfg["hasApiKey"], json!(true));

        let (status, _) =
            put(json!({ "enabled": false, "provider": "openai", "baseUrl": "https://b.example.com/v1" }))
                .await;
        assert_eq!(status, StatusCode::OK);
        let (_, cfg) = send_json(
            &app,
            "GET",
            "/api/assistant/config",
            Some(&admin),
            json!({}),
        )
        .await;
        assert_eq!(cfg["hasApiKey"], json!(false));
    }

    #[tokio::test]
    async fn custom_widget_reads_search_engine_and_settings_validation() {
        let (_state, app) = setup_test_state().await;
        for uri in ["/api/custom-widgets", "/api/custom-widgets/missing"] {
            let (status, _) = send_json(&app, "GET", uri, None, json!({})).await;
            assert!(
                matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
                "{uri}: {status}"
            );
        }

        let admin = login_cookie(&app, "demo", "demo").await;
        create_regular_user(&app, &admin, "cwviewer", "viewerpass1").await;
        let viewer = login_cookie(&app, "cwviewer", "viewerpass1").await;
        let (status, _) =
            send_json(&app, "GET", "/api/custom-widgets", Some(&viewer), json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let engine = |tpl: &str| json!({ "name": "X", "short": "x", "iconUrl": "/i.svg", "urlTemplate": tpl, "type": "generic" });
        for tpl in [
            "javascript:alert(1)//%s",
            "https://example.com/?q=",
            "ftp://e.com/%s",
        ] {
            let (status, _) = send_json(
                &app,
                "POST",
                "/api/search-engines",
                Some(&admin),
                engine(tpl),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{tpl}");
        }
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/search-engines",
            Some(&admin),
            engine("https://example.com/?q=%s"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        for patch in [
            json!({ "notASetting": { "a": 1 } }),
            json!({ "user": { "enableGravatar": "yes" } }),
            json!({ "board": { "homeBoardId": "does-not-exist" } }),
            json!({ "analytics": { "instanceId": "x" } }),
        ] {
            let (status, _) =
                send_json(&app, "PATCH", "/api/settings", Some(&admin), patch.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{patch}");
        }
        let (status, body) = send_json(
            &app,
            "PATCH",
            "/api/settings",
            Some(&admin),
            json!({ "user": { "enableGravatar": false } }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["user"]["enableGravatar"], json!(false));
    }

    #[tokio::test]
    async fn section_collapse_is_per_user_and_requires_auth() {
        let (state, app) = setup_test_state().await;
        let admin = login_cookie(&app, "demo", "demo").await;
        let board = repos::boards::create_board_with_defaults(
            &state.db,
            repos::boards::CreateBoardInput {
                id: None,
                name: "collapse-board".into(),
                is_public: false,
                creator_id: None,
                column_count: 10,
            },
        )
        .await
        .unwrap();
        let board_id = board.board.id.clone();
        let mut sections: Vec<repos::boards::SaveBoardSectionInput> = board
            .sections
            .iter()
            .map(|s| repos::boards::SaveBoardSectionInput {
                id: s.id.clone(),
                kind: s.kind.clone(),
                x_offset: s.x_offset,
                y_offset: s.y_offset,
                name: s.name.clone(),
                options: s.options.clone(),
                layouts: s.layouts.clone(),
            })
            .collect();
        sections.push(repos::boards::SaveBoardSectionInput {
            id: "container-1".into(),
            kind: "container".into(),
            x_offset: None,
            y_offset: None,
            name: None,
            options: json!({ "title": "Media", "collapsible": true }),
            layouts: Vec::new(),
        });
        repos::boards::save_board_content(&state.db, &board_id, &sections, &[])
            .await
            .unwrap();
        let uri = format!("/api/boards/{board_id}/sections/container-1/collapse");

        let (status, _) = send_json(&app, "PUT", &uri, None, json!({ "collapsed": true })).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = send_json(
            &app,
            "PUT",
            &uri,
            Some(&admin),
            json!({ "collapsed": true }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
        let demo = repos::users::find_by_name_ci(&state.db, "demo")
            .await
            .unwrap()
            .unwrap();
        let states = repos::boards::get_section_collapse_states(&state.db, &demo.id)
            .await
            .unwrap();
        assert_eq!(states.get("container-1"), Some(&true));
    }
}
