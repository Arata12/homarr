use std::path::Path;
use std::time::Duration;

use axum::{extract::DefaultBodyLimit, Router};
use homarr_api::{api_router, AppState};
use homarr_common::SecretCrypto;
use homarr_core::{init_logging, AppConfig, CertificateManager, DatabaseDriver, HttpClientFactory};
use homarr_db::{
    seed::{seed_defaults, seed_demo},
    Database,
};
use homarr_redis::RedisStore;
use homarr_web::web_router;
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
};

fn ensure_sqlite_parent_dir(driver: DatabaseDriver, url: &str) {
    if driver != DatabaseDriver::Sqlite {
        return;
    }
    let clean = url
        .trim()
        .strip_prefix("sqlite://")
        .or_else(|| url.trim().strip_prefix("sqlite:"))
        .unwrap_or(url.trim());
    if clean.is_empty() || clean == ":memory:" || clean.starts_with("mode=memory") {
        return;
    }
    if let Some(parent) = Path::new(clean).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::warn!(path = %parent.display(), error = %err, "Failed to create SQLite parent directory");
            }
        }
    }
}

async fn connect_redis_with_retry(config: &AppConfig) -> RedisStore {
    if config.redis_disabled {
        return RedisStore::in_memory();
    }
    for attempt in 1..=5 {
        match RedisStore::connect(config).await {
            Ok(store) => {
                if store.health_check().await.is_ok() {
                    return store;
                }
            }
            Err(err) => {
                tracing::warn!(
                    attempt = attempt,
                    error = %err,
                    "Redis connection attempt failed"
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tracing::warn!("Falling back to in-memory Redis store");
    RedisStore::in_memory()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;
    let logs = init_logging(&config.log_level);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        host = %config.host,
        port = config.port,
        "Starting Homarr (Rust Edition)"
    );

    let crypto = SecretCrypto::new(&config.secret_encryption_key)?;
    let certs = CertificateManager::new(config.local_certificate_path.clone());
    let http = HttpClientFactory::new(&certs)?;

    ensure_sqlite_parent_dir(config.db_driver, &config.db_url);
    let db = Database::connect(config.db_driver, &config.db_url).await?;
    db.migrate().await?;

    if config.demo_mode {
        seed_demo(&db, &crypto).await?;
    } else {
        seed_defaults(&db).await?;
    }

    let redis = connect_redis_with_retry(&config).await;
    let state = AppState::build(config.clone(), db, redis, crypto, logs, certs, http);
    state.scheduler.start().await?;

    let assets = &config.assets_dir;
    let app: Router = api_router(state.clone())
        .merge(web_router(state.clone()))
        .nest_service("/static", ServeDir::new(assets.join("static")))
        .nest_service("/public", ServeDir::new(assets.join("public")))
        .nest_service("/images", ServeDir::new(assets.join("public/images")))
        .nest_service("/logo", ServeDir::new(assets.join("public/logo")))
        .route_service(
            "/favicon.ico",
            ServeFile::new(assets.join("public/favicon.ico")),
        )
        .route_service(
            "/logo.png",
            ServeFile::new(assets.join("public/logo/logo.png")),
        )
        .route_service("/sw.js", ServeFile::new(assets.join("public/sw.js")))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .layer(CompressionLayer::new().gzip(true));

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(listen_addr = %addr, "Homarr listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    state.scheduler.shutdown().await;
    tracing::info!("Homarr shutdown complete");
    Ok(())
}
