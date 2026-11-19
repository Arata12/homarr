use std::path::{Path, PathBuf};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use homarr_auth::{hash_password, verify_password};
use homarr_common::{create_id, generate_secure_random_token, HomarrError, HomarrResult};
use homarr_services::validate_custom_widget_definition;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer, services::ServeDir, set_header::SetResponseHeaderLayer,
};

#[derive(Clone)]
struct WorkshopState {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListingRecord {
    id: String,
    slug: String,
    name: String,
    description: Option<String>,
    icon_url: Option<String>,
    author: Option<String>,
    version: String,
    downloads: i64,
    definition: Value,
    created_at: i64,
    updated_at: i64,
}

async fn init_workshop_db(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    if db_url != ":memory:" && !db_url.starts_with("sqlite::memory:") {
        let clean = db_url
            .strip_prefix("sqlite://")
            .or_else(|| db_url.strip_prefix("sqlite:"))
            .unwrap_or(db_url);
        if let Some(parent) = Path::new(clean).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
    }

    let is_memory = db_url == ":memory:" || db_url == "sqlite::memory:";
    let opts = if is_memory {
        SqliteConnectOptions::new()
            .filename(":memory:")
            .create_if_missing(true)
    } else {
        let clean = db_url
            .strip_prefix("sqlite://")
            .or_else(|| db_url.strip_prefix("sqlite:"))
            .unwrap_or(db_url);
        SqliteConnectOptions::new()
            .filename(clean)
            .create_if_missing(true)
    };

    let pool = SqlitePoolOptions::new()
        .max_connections(if is_memory { 1 } else { 5 })
        .connect_with(opts)
        .await?;

    sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS "workshop_users" (
            "id" TEXT PRIMARY KEY NOT NULL,
            "username" TEXT NOT NULL UNIQUE,
            "password_hash" TEXT NOT NULL,
            "token" TEXT,
            "created_at" INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS "workshop_listings" (
            "id" TEXT PRIMARY KEY NOT NULL,
            "slug" TEXT NOT NULL UNIQUE,
            "name" TEXT NOT NULL,
            "description" TEXT,
            "icon_url" TEXT,
            "author" TEXT,
            "version" TEXT NOT NULL,
            "downloads" INTEGER NOT NULL DEFAULT 0,
            "definition_json" TEXT NOT NULL,
            "created_at" INTEGER NOT NULL,
            "updated_at" INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    // Older databases predate listing ownership; add the column in place.
    let has_author_id: i64 = sqlx::query(
        r#"SELECT count(*) AS cnt FROM pragma_table_info('workshop_listings') WHERE name = 'author_id'"#,
    )
    .fetch_one(&pool)
    .await?
    .try_get("cnt")?;
    if has_author_id == 0 {
        sqlx::raw_sql(r#"ALTER TABLE "workshop_listings" ADD COLUMN "author_id" TEXT"#)
            .execute(&pool)
            .await?;
    }
    sqlx::raw_sql(
        r#"CREATE INDEX IF NOT EXISTS "workshop_users_token_idx" ON "workshop_users" ("token")"#,
    )
    .execute(&pool)
    .await?;

    let count: i64 = sqlx::query(r#"SELECT count(*) as cnt FROM "workshop_listings""#)
        .fetch_one(&pool)
        .await?
        .try_get("cnt")?;

    if count == 0 {
        let now = Utc::now().timestamp();
        let def = json!({
            "schema": "homarr-custom-widget-v2",
            "name": "Homarr Welcome Card",
            "description": "Sample Custom Widget v2 package hosted on Homarr Workshop",
            "sources": [],
            "requests": [],
            "options": [],
            "template": "<div class=\"card\"><strong>{{title}}</strong><p>Installed from Workshop</p></div>"
        });
        sqlx::query(
            r#"INSERT INTO "workshop_listings" ("id", "slug", "name", "description", "icon_url", "author", "version", "downloads", "definition_json", "created_at", "updated_at")
               VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"#,
        )
        .bind(create_id())
        .bind("homarr-welcome-card")
        .bind("Homarr Welcome Card")
        .bind("Sample Custom Widget v2 package hosted on Homarr Workshop")
        .bind("/logo/logo.png")
        .bind("Homarr Team")
        .bind("1.0.0")
        .bind(serde_json::to_string(&def).unwrap_or_default())
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await?;
    }

    Ok(pool)
}

async fn health() -> Json<Value> {
    Json(
        json!({ "status": "ok", "service": "homarr-workshop", "version": env!("CARGO_PKG_VERSION") }),
    )
}

#[derive(Deserialize)]
struct ListingsQuery {
    q: Option<String>,
}

async fn list_listings(
    State(state): State<WorkshopState>,
    Query(q): Query<ListingsQuery>,
) -> HomarrResult<Json<Value>> {
    let rows = sqlx::query(
        r#"SELECT * FROM "workshop_listings" ORDER BY "downloads" DESC, "updated_at" DESC"#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| HomarrError::Database(e.to_string()))?;

    let needle = q.q.unwrap_or_default().trim().to_ascii_lowercase();
    let mut items = Vec::new();
    for r in rows {
        let def_str: String = r
            .try_get("definition_json")
            .map_err(|e| HomarrError::Database(e.to_string()))?;
        let rec = ListingRecord {
            id: r
                .try_get("id")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            slug: r
                .try_get("slug")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            name: r
                .try_get("name")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            description: r
                .try_get("description")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            icon_url: r
                .try_get("icon_url")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            author: r
                .try_get("author")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            version: r
                .try_get("version")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            downloads: r
                .try_get("downloads")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            definition: serde_json::from_str(&def_str).unwrap_or(Value::Null),
            created_at: r
                .try_get("created_at")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
            updated_at: r
                .try_get("updated_at")
                .map_err(|e| HomarrError::Database(e.to_string()))?,
        };
        if !needle.is_empty()
            && !rec.name.to_ascii_lowercase().contains(&needle)
            && !rec
                .description
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains(&needle)
        {
            continue;
        }
        items.push(rec);
    }
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateListingBody {
    pub slug: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub version: Option<String>,
    pub definition: Value,
}

struct WorkshopUser {
    id: String,
    username: String,
}

/// Resolves `Authorization: Bearer <token>` to the account that login/register issued it to.
/// The token is matched by an indexed DB lookup, not compared in Rust code.
async fn authenticate(pool: &SqlitePool, headers: &HeaderMap) -> HomarrResult<WorkshopUser> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| HomarrError::Unauthorized("Missing bearer token".into()))?;
    let row = sqlx::query(r#"SELECT "id", "username" FROM "workshop_users" WHERE "token" = ?"#)
        .bind(token)
        .fetch_optional(pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?
        .ok_or_else(|| HomarrError::Unauthorized("Invalid token".into()))?;
    Ok(WorkshopUser {
        id: row
            .try_get("id")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        username: row
            .try_get("username")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
    })
}

async fn create_listing(
    State(state): State<WorkshopState>,
    headers: HeaderMap,
    Json(body): Json<CreateListingBody>,
) -> HomarrResult<(StatusCode, Json<ListingRecord>)> {
    let user = authenticate(&state.pool, &headers).await?;
    validate_custom_widget_definition(&body.definition)?;
    let id = create_id();
    let now = Utc::now().timestamp();
    let slug = body.slug.unwrap_or_else(|| {
        body.name
            .trim()
            .to_ascii_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
    });
    let version = body.version.unwrap_or_else(|| "1.0.0".to_string());
    let def_json = serde_json::to_string(&body.definition)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;

    sqlx::query(
        r#"INSERT INTO "workshop_listings" ("id", "slug", "name", "description", "icon_url", "author", "author_id", "version", "downloads", "definition_json", "created_at", "updated_at")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"#,
    )
    .bind(&id)
    .bind(&slug)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(&body.icon_url)
    .bind(&user.username)
    .bind(&user.id)
    .bind(&version)
    .bind(&def_json)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| HomarrError::Database(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(ListingRecord {
            id,
            slug,
            name: body.name.trim().to_string(),
            description: body.description,
            icon_url: body.icon_url,
            author: Some(user.username),
            version,
            downloads: 0,
            definition: body.definition,
            created_at: now,
            updated_at: now,
        }),
    ))
}

async fn get_listing(
    State(state): State<WorkshopState>,
    AxumPath(id): AxumPath<String>,
) -> HomarrResult<Json<ListingRecord>> {
    let row = sqlx::query(r#"SELECT * FROM "workshop_listings" WHERE "id" = ? OR "slug" = ?"#)
        .bind(&id)
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?
        .ok_or_else(|| HomarrError::NotFound("Listing not found".into()))?;

    let def_str: String = row
        .try_get("definition_json")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    Ok(Json(ListingRecord {
        id: row
            .try_get("id")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        slug: row
            .try_get("slug")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        name: row
            .try_get("name")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        description: row
            .try_get("description")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        icon_url: row
            .try_get("icon_url")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        author: row
            .try_get("author")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        version: row
            .try_get("version")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        downloads: row
            .try_get("downloads")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        definition: serde_json::from_str(&def_str).unwrap_or(Value::Null),
        created_at: row
            .try_get("created_at")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|e| HomarrError::Database(e.to_string()))?,
    }))
}

async fn download_listing(
    State(state): State<WorkshopState>,
    AxumPath(id): AxumPath<String>,
) -> HomarrResult<Json<Value>> {
    let row = sqlx::query(
        r#"SELECT "id", "definition_json" FROM "workshop_listings" WHERE "id" = ? OR "slug" = ?"#,
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| HomarrError::Database(e.to_string()))?
    .ok_or_else(|| HomarrError::NotFound("Listing not found".into()))?;
    let real_id: String = row
        .try_get("id")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    let def_str: String = row
        .try_get("definition_json")
        .map_err(|e| HomarrError::Database(e.to_string()))?;

    sqlx::query(r#"UPDATE "workshop_listings" SET "downloads" = "downloads" + 1 WHERE "id" = ?"#)
        .bind(&real_id)
        .execute(&state.pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?;

    let def: Value = serde_json::from_str(&def_str)
        .map_err(|e| HomarrError::Internal(format!("Invalid stored definition JSON: {e}")))?;
    Ok(Json(def))
}

async fn delete_listing(
    State(state): State<WorkshopState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> HomarrResult<Json<Value>> {
    let user = authenticate(&state.pool, &headers).await?;
    let row = sqlx::query(
        r#"SELECT "id", "author_id" FROM "workshop_listings" WHERE "id" = ? OR "slug" = ?"#,
    )
    .bind(&id)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| HomarrError::Database(e.to_string()))?
    .ok_or_else(|| HomarrError::NotFound("Listing not found".into()))?;
    let real_id: String = row
        .try_get("id")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    let author_id: Option<String> = row
        .try_get("author_id")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    // No admin role exists in this service, so only the owning account may delete.
    if author_id.as_deref() != Some(user.id.as_str()) {
        return Err(HomarrError::Forbidden(
            "Only the listing author can delete it".into(),
        ));
    }

    sqlx::query(r#"DELETE FROM "workshop_listings" WHERE "id" = ?"#)
        .bind(&real_id)
        .execute(&state.pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct AuthBody {
    username: String,
    password: String,
}

async fn auth_register(
    State(state): State<WorkshopState>,
    Json(body): Json<AuthBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    let uname = body.username.trim().to_string();
    if uname.len() < 2 || body.password.len() < 4 {
        return Err(HomarrError::BadRequest(
            "Username >= 2 and password >= 4 required".into(),
        ));
    }
    let id = create_id();
    let hash = hash_password(&body.password).await?;
    let token = generate_secure_random_token(24);
    sqlx::query(
        r#"INSERT INTO "workshop_users" ("id", "username", "password_hash", "token", "created_at") VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind(&id)
    .bind(&uname)
    .bind(&hash)
    .bind(&token)
    .bind(Utc::now().timestamp())
    .execute(&state.pool)
    .await
    .map_err(|_| HomarrError::Conflict("Username already exists".into()))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "username": uname, "token": token })),
    ))
}

async fn auth_login(
    State(state): State<WorkshopState>,
    Json(body): Json<AuthBody>,
) -> HomarrResult<Json<Value>> {
    let row = sqlx::query(r#"SELECT * FROM "workshop_users" WHERE "username" = ?"#)
        .bind(body.username.trim())
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?
        .ok_or_else(|| HomarrError::Unauthorized("Invalid credentials".into()))?;
    let hash: String = row
        .try_get("password_hash")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    if !verify_password(&body.password, &hash).await? {
        return Err(HomarrError::Unauthorized("Invalid credentials".into()));
    }
    let id: String = row
        .try_get("id")
        .map_err(|e| HomarrError::Database(e.to_string()))?;
    let token = generate_secure_random_token(24);
    sqlx::query(r#"UPDATE "workshop_users" SET "token" = ? WHERE "id" = ?"#)
        .bind(&token)
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| HomarrError::Database(e.to_string()))?;

    Ok(Json(
        json!({ "id": id, "username": body.username.trim(), "token": token }),
    ))
}

fn workshop_router(state: WorkshopState, public_dir: &Path) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/listings", get(list_listings).post(create_listing))
        .route(
            "/api/listings/{id}",
            get(get_listing).delete(delete_listing),
        )
        .route("/api/listings/{id}/download", get(download_listing))
        .route("/api/auth/register", post(auth_register))
        .route("/api/auth/login", post(auth_login))
        .with_state(state)
        .layer(CompressionLayer::new().gzip(true));

    if public_dir.exists() {
        api.fallback_service(ServeDir::new(public_dir))
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            ))
    } else {
        api
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8090);
    let db_url =
        std::env::var("WORKSHOP_DB_URL").unwrap_or_else(|_| "/data/workshop.sqlite".to_string());
    let public_dir = PathBuf::from(
        std::env::var("WORKSHOP_PUBLIC_DIR").unwrap_or_else(|_| "/app/public".to_string()),
    );

    let pool = init_workshop_db(&db_url).await?;
    let app = workshop_router(WorkshopState { pool }, &public_dir);

    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(listen_addr = %addr, "Homarr Workshop listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn register(app: &Router, username: &str) -> String {
        let res = app
            .clone()
            .oneshot(
                Request::post("/api/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "username": username, "password": "secret-pw" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let v: Value =
            serde_json::from_slice(&to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        v["token"].as_str().unwrap().to_string()
    }

    fn create_request(token: Option<&str>) -> Request<Body> {
        let mut req =
            Request::post("/api/listings").header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        req.body(Body::from(
            json!({
                "name": "Owned Tile",
                "author": "Homarr Team",
                "definition": {
                    "schema": "homarr-custom-widget-v2",
                    "name": "Owned Tile",
                    "template": "<div>{{value}}</div>",
                    "sources": [],
                    "requests": []
                }
            })
            .to_string(),
        ))
        .unwrap()
    }

    fn delete_request(id: &str, token: &str) -> Request<Body> {
        Request::delete(format!("/api/listings/{id}"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn listing_writes_require_token_and_ownership() {
        let pool = init_workshop_db("sqlite::memory:").await.unwrap();
        let app = workshop_router(WorkshopState { pool }, Path::new("/nonexistent"));

        let anon = app.clone().oneshot(create_request(None)).await.unwrap();
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
        let bogus = app
            .clone()
            .oneshot(create_request(Some("not-a-real-token")))
            .await
            .unwrap();
        assert_eq!(bogus.status(), StatusCode::UNAUTHORIZED);

        let alice = register(&app, "alice").await;
        let mallory = register(&app, "mallory").await;

        let created = app
            .clone()
            .oneshot(create_request(Some(&alice)))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let rec: Value =
            serde_json::from_slice(&to_bytes(created.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        // Client-supplied author is ignored.
        assert_eq!(rec["author"], "alice");
        let id = rec["id"].as_str().unwrap().to_string();

        let anon_delete = app
            .clone()
            .oneshot(
                Request::delete(format!("/api/listings/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anon_delete.status(), StatusCode::UNAUTHORIZED);

        let foreign = app
            .clone()
            .oneshot(delete_request(&id, &mallory))
            .await
            .unwrap();
        assert_eq!(foreign.status(), StatusCode::FORBIDDEN);

        // Seeded listing has no owner account, so nobody can delete it.
        let seed = app
            .clone()
            .oneshot(delete_request("homarr-welcome-card", &mallory))
            .await
            .unwrap();
        assert_eq!(seed.status(), StatusCode::FORBIDDEN);

        let own = app
            .clone()
            .oneshot(delete_request(&id, &alice))
            .await
            .unwrap();
        assert_eq!(own.status(), StatusCode::OK);

        let gone = app
            .oneshot(
                Request::get(format!("/api/listings/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn workshop_health_create_validate_and_download() {
        let pool = init_workshop_db("sqlite::memory:").await.unwrap();
        let app = workshop_router(WorkshopState { pool }, Path::new("/nonexistent"));

        let h = app
            .clone()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(h.status(), StatusCode::OK);

        let token = register(&app, "alice").await;

        // Reject unsafe Custom Widget v2 definition containing <script>
        let bad = app
            .clone()
            .oneshot(
                Request::post("/api/listings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "name": "Evil",
                            "definition": {
                                "schema": "homarr-custom-widget-v2",
                                "name": "Evil",
                                "template": "<script>alert(1)</script>"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

        // Create valid listing and download it
        let ok = app
            .clone()
            .oneshot(
                Request::post("/api/listings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "name": "Metrics Tile",
                            "definition": {
                                "schema": "homarr-custom-widget-v2",
                                "name": "Metrics Tile",
                                "template": "<div>{{value}}</div>",
                                "sources": [],
                                "requests": []
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::CREATED);
        let created: Value =
            serde_json::from_slice(&to_bytes(ok.into_body(), usize::MAX).await.unwrap()).unwrap();
        let id = created["id"].as_str().unwrap();

        let dl = app
            .clone()
            .oneshot(
                Request::get(format!("/api/listings/{id}/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(dl.status(), StatusCode::OK);

        let fetched = app
            .oneshot(
                Request::get(format!("/api/listings/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let rec: Value =
            serde_json::from_slice(&to_bytes(fetched.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(rec["downloads"], 1);
    }
}
