use homarr_common::{create_id, HomarrResult};
use sqlx::Row;

use crate::models::{ApiKeyRecord, ApiKeyWithUser};
use crate::pool::map_sqlx_err;
use crate::repos::{ms_to_naive, naive_to_ms};
use crate::Database;

pub struct CreateApiKeyInput {
    pub user_id: String,
    pub hashed_secret: String,
    pub read_only: bool,
    pub oauth_client_id: Option<String>,
    pub oauth_client_name: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_issued_at_ms: Option<i64>,
}

pub async fn create(db: &Database, input: CreateApiKeyInput) -> HomarrResult<ApiKeyRecord> {
    let id = create_id();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "apiKey" ("id", "api_key", "read_only", "oauth_client_id", "oauth_client_name", "oauth_redirect_uri", "oauth_issued_at", "user_id")
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.hashed_secret)
            .bind(input.read_only)
            .bind(&input.oauth_client_id)
            .bind(&input.oauth_client_name)
            .bind(&input.oauth_redirect_uri)
            .bind(input.oauth_issued_at_ms)
            .bind(&input.user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "apiKey" ("id", "api_key", "read_only", "oauth_client_id", "oauth_client_name", "oauth_redirect_uri", "oauth_issued_at", "user_id")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            )
            .bind(&id)
            .bind(&input.hashed_secret)
            .bind(input.read_only)
            .bind(&input.oauth_client_id)
            .bind(&input.oauth_client_name)
            .bind(&input.oauth_redirect_uri)
            .bind(input.oauth_issued_at_ms.map(ms_to_naive))
            .bind(&input.user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    Ok(ApiKeyRecord {
        id,
        api_key: input.hashed_secret,
        read_only: input.read_only,
        oauth_client_id: input.oauth_client_id,
        oauth_client_name: input.oauth_client_name,
        oauth_redirect_uri: input.oauth_redirect_uri,
        oauth_issued_at_ms: input.oauth_issued_at_ms,
        user_id: input.user_id,
    })
}

pub async fn find_by_id_with_user(db: &Database, id: &str) -> HomarrResult<Option<ApiKeyWithUser>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(
                r#"SELECT k.*, u."name" as user_name, u."email" as user_email, u."color_scheme" as user_color_scheme
                   FROM "apiKey" k
                   INNER JOIN "user" u ON u."id" = k."user_id"
                   WHERE k."id" = ?"#,
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?;

            let Some(r) = row else {
                return Ok(None);
            };
            Ok(Some(ApiKeyWithUser {
                api_key: ApiKeyRecord {
                    id: r.try_get("id").map_err(map_sqlx_err)?,
                    api_key: r.try_get("api_key").map_err(map_sqlx_err)?,
                    read_only: r.try_get("read_only").map_err(map_sqlx_err)?,
                    oauth_client_id: r.try_get("oauth_client_id").map_err(map_sqlx_err)?,
                    oauth_client_name: r.try_get("oauth_client_name").map_err(map_sqlx_err)?,
                    oauth_redirect_uri: r.try_get("oauth_redirect_uri").map_err(map_sqlx_err)?,
                    oauth_issued_at_ms: r.try_get("oauth_issued_at").map_err(map_sqlx_err)?,
                    user_id: r.try_get("user_id").map_err(map_sqlx_err)?,
                },
                user_name: r.try_get("user_name").map_err(map_sqlx_err)?,
                user_email: r.try_get("user_email").map_err(map_sqlx_err)?,
                user_color_scheme: r.try_get("user_color_scheme").map_err(map_sqlx_err)?,
            }))
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(
                r#"SELECT k.*, u."name" as user_name, u."email" as user_email, u."color_scheme" as user_color_scheme
                   FROM "apiKey" k
                   INNER JOIN "user" u ON u."id" = k."user_id"
                   WHERE k."id" = $1"#,
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?;

            let Some(r) = row else {
                return Ok(None);
            };
            let issued: Option<chrono::NaiveDateTime> =
                r.try_get("oauth_issued_at").map_err(map_sqlx_err)?;
            Ok(Some(ApiKeyWithUser {
                api_key: ApiKeyRecord {
                    id: r.try_get("id").map_err(map_sqlx_err)?,
                    api_key: r.try_get("api_key").map_err(map_sqlx_err)?,
                    read_only: r.try_get("read_only").map_err(map_sqlx_err)?,
                    oauth_client_id: r.try_get("oauth_client_id").map_err(map_sqlx_err)?,
                    oauth_client_name: r.try_get("oauth_client_name").map_err(map_sqlx_err)?,
                    oauth_redirect_uri: r.try_get("oauth_redirect_uri").map_err(map_sqlx_err)?,
                    oauth_issued_at_ms: issued.map(naive_to_ms),
                    user_id: r.try_get("user_id").map_err(map_sqlx_err)?,
                },
                user_name: r.try_get("user_name").map_err(map_sqlx_err)?,
                user_email: r.try_get("user_email").map_err(map_sqlx_err)?,
                user_color_scheme: r.try_get("user_color_scheme").map_err(map_sqlx_err)?,
            }))
        }
    }
}

pub async fn list_by_user(db: &Database, user_id: &str) -> HomarrResult<Vec<ApiKeyRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "apiKey" WHERE "user_id" = ?"#)
                .bind(user_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(ApiKeyRecord {
                        id: r.try_get("id")?,
                        api_key: r.try_get("api_key")?,
                        read_only: r.try_get("read_only")?,
                        oauth_client_id: r.try_get("oauth_client_id")?,
                        oauth_client_name: r.try_get("oauth_client_name")?,
                        oauth_redirect_uri: r.try_get("oauth_redirect_uri")?,
                        oauth_issued_at_ms: r.try_get("oauth_issued_at")?,
                        user_id: r.try_get("user_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "apiKey" WHERE "user_id" = $1"#)
                .bind(user_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let issued: Option<chrono::NaiveDateTime> = r.try_get("oauth_issued_at")?;
                    Ok(ApiKeyRecord {
                        id: r.try_get("id")?,
                        api_key: r.try_get("api_key")?,
                        read_only: r.try_get("read_only")?,
                        oauth_client_id: r.try_get("oauth_client_id")?,
                        oauth_client_name: r.try_get("oauth_client_name")?,
                        oauth_redirect_uri: r.try_get("oauth_redirect_uri")?,
                        oauth_issued_at_ms: issued.map(naive_to_ms),
                        user_id: r.try_get("user_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "apiKey" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "apiKey" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
