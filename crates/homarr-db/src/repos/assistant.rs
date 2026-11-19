use chrono::Utc;
use homarr_common::{create_id, HomarrResult};
use sqlx::Row;

use crate::models::{AssistantConfigRecord, AssistantMessageRecord, AssistantThreadRecord};
use crate::pool::map_sqlx_err;
use crate::repos::{naive_to_secs, secs_to_naive};
use crate::Database;

pub async fn get_config(db: &Database) -> HomarrResult<AssistantConfigRecord> {
    let found = match db {
        Database::Sqlite(pool) => {
            let row =
                sqlx::query(r#"SELECT * FROM "assistant_configuration" WHERE "id" = 'default'"#)
                    .fetch_optional(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(AssistantConfigRecord {
                    id: r.try_get("id")?,
                    enabled: r.try_get("enabled")?,
                    web_search_enabled: r.try_get("web_search_enabled")?,
                    provider: r.try_get("provider")?,
                    base_url: r.try_get("base_url")?,
                    model_discovery_path: r.try_get("model_discovery_path")?,
                    encrypted_api_key: r.try_get("encrypted_api_key")?,
                    encrypted_headers: r.try_get("encrypted_headers")?,
                    model_id: r.try_get("model_id")?,
                    updated_at: r.try_get("updated_at")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let row =
                sqlx::query(r#"SELECT * FROM "assistant_configuration" WHERE "id" = 'default'"#)
                    .fetch_optional(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            row.map(|r| {
                let dt: chrono::NaiveDateTime = r.try_get("updated_at")?;
                Ok(AssistantConfigRecord {
                    id: r.try_get("id")?,
                    enabled: r.try_get("enabled")?,
                    web_search_enabled: r.try_get("web_search_enabled")?,
                    provider: r.try_get("provider")?,
                    base_url: r.try_get("base_url")?,
                    model_discovery_path: r.try_get("model_discovery_path")?,
                    encrypted_api_key: r.try_get("encrypted_api_key")?,
                    encrypted_headers: r.try_get("encrypted_headers")?,
                    model_id: r.try_get("model_id")?,
                    updated_at: naive_to_secs(dt),
                })
            })
            .transpose()
            .map_err(map_sqlx_err)?
        }
    };

    Ok(found.unwrap_or_else(|| AssistantConfigRecord {
        id: "default".to_string(),
        enabled: false,
        web_search_enabled: false,
        provider: "openrouter".to_string(),
        base_url: "https://openrouter.ai/api/v1".to_string(),
        model_discovery_path: Some("/models".to_string()),
        encrypted_api_key: None,
        encrypted_headers: None,
        model_id: None,
        updated_at: Utc::now().timestamp(),
    }))
}

pub async fn save_config(db: &Database, cfg: &AssistantConfigRecord) -> HomarrResult<()> {
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "assistant_configuration"
                   ("id", "enabled", "web_search_enabled", "provider", "base_url", "model_discovery_path", "encrypted_api_key", "encrypted_headers", "model_id", "updated_at")
                   VALUES ('default', ?, ?, ?, ?, ?, ?, ?, ?, ?)
                   ON CONFLICT ("id") DO UPDATE SET
                   "enabled" = excluded."enabled", "web_search_enabled" = excluded."web_search_enabled",
                   "provider" = excluded."provider", "base_url" = excluded."base_url",
                   "model_discovery_path" = excluded."model_discovery_path",
                   "encrypted_api_key" = coalesce(excluded."encrypted_api_key", "assistant_configuration"."encrypted_api_key"),
                   "encrypted_headers" = coalesce(excluded."encrypted_headers", "assistant_configuration"."encrypted_headers"),
                   "model_id" = excluded."model_id", "updated_at" = excluded."updated_at""#,
            )
            .bind(cfg.enabled)
            .bind(cfg.web_search_enabled)
            .bind(&cfg.provider)
            .bind(&cfg.base_url)
            .bind(&cfg.model_discovery_path)
            .bind(&cfg.encrypted_api_key)
            .bind(&cfg.encrypted_headers)
            .bind(&cfg.model_id)
            .bind(now)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "assistant_configuration"
                   ("id", "enabled", "web_search_enabled", "provider", "base_url", "model_discovery_path", "encrypted_api_key", "encrypted_headers", "model_id", "updated_at")
                   VALUES ('default', $1, $2, $3, $4, $5, $6, $7, $8, $9)
                   ON CONFLICT ("id") DO UPDATE SET
                   "enabled" = excluded."enabled", "web_search_enabled" = excluded."web_search_enabled",
                   "provider" = excluded."provider", "base_url" = excluded."base_url",
                   "model_discovery_path" = excluded."model_discovery_path",
                   "encrypted_api_key" = coalesce(excluded."encrypted_api_key", "assistant_configuration"."encrypted_api_key"),
                   "encrypted_headers" = coalesce(excluded."encrypted_headers", "assistant_configuration"."encrypted_headers"),
                   "model_id" = excluded."model_id", "updated_at" = excluded."updated_at""#,
            )
            .bind(cfg.enabled)
            .bind(cfg.web_search_enabled)
            .bind(&cfg.provider)
            .bind(&cfg.base_url)
            .bind(&cfg.model_discovery_path)
            .bind(&cfg.encrypted_api_key)
            .bind(&cfg.encrypted_headers)
            .bind(&cfg.model_id)
            .bind(secs_to_naive(now))
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn create_thread(
    db: &Database,
    user_id: &str,
    title: Option<&str>,
    model_id: Option<&str>,
) -> HomarrResult<AssistantThreadRecord> {
    let id = create_id();
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "assistant_thread" ("id", "user_id", "title", "model_id", "created_at", "updated_at")
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(user_id)
            .bind(title)
            .bind(model_id)
            .bind(now)
            .bind(now)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let dt = secs_to_naive(now);
            sqlx::query(
                r#"INSERT INTO "assistant_thread" ("id", "user_id", "title", "model_id", "created_at", "updated_at")
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(&id)
            .bind(user_id)
            .bind(title)
            .bind(model_id)
            .bind(dt)
            .bind(dt)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(AssistantThreadRecord {
        id,
        user_id: user_id.to_string(),
        title: title.map(str::to_string),
        model_id: model_id.map(str::to_string),
        created_at: now,
        updated_at: now,
    })
}

pub async fn list_threads(
    db: &Database,
    user_id: &str,
) -> HomarrResult<Vec<AssistantThreadRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "assistant_thread" WHERE "user_id" = ? ORDER BY "updated_at" DESC"#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(AssistantThreadRecord {
                        id: r.try_get("id")?,
                        user_id: r.try_get("user_id")?,
                        title: r.try_get("title")?,
                        model_id: r.try_get("model_id")?,
                        created_at: r.try_get("created_at")?,
                        updated_at: r.try_get("updated_at")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "assistant_thread" WHERE "user_id" = $1 ORDER BY "updated_at" DESC"#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let c: chrono::NaiveDateTime = r.try_get("created_at")?;
                    let u: chrono::NaiveDateTime = r.try_get("updated_at")?;
                    Ok(AssistantThreadRecord {
                        id: r.try_get("id")?,
                        user_id: r.try_get("user_id")?,
                        title: r.try_get("title")?,
                        model_id: r.try_get("model_id")?,
                        created_at: naive_to_secs(c),
                        updated_at: naive_to_secs(u),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn add_message(
    db: &Database,
    thread_id: &str,
    parent_id: Option<&str>,
    content: &str,
) -> HomarrResult<AssistantMessageRecord> {
    let id = create_id();
    let now = Utc::now().timestamp();
    let format = "rust/v1";
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "assistant_message" ("id", "thread_id", "parent_id", "format", "content", "created_at")
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(thread_id)
            .bind(parent_id)
            .bind(format)
            .bind(content)
            .bind(now)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(r#"UPDATE "assistant_thread" SET "updated_at" = ? WHERE "id" = ?"#)
                .bind(now)
                .bind(thread_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let dt = secs_to_naive(now);
            sqlx::query(
                r#"INSERT INTO "assistant_message" ("id", "thread_id", "parent_id", "format", "content", "created_at")
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(&id)
            .bind(thread_id)
            .bind(parent_id)
            .bind(format)
            .bind(content)
            .bind(dt)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(r#"UPDATE "assistant_thread" SET "updated_at" = $1 WHERE "id" = $2"#)
                .bind(dt)
                .bind(thread_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(AssistantMessageRecord {
        id,
        thread_id: thread_id.to_string(),
        parent_id: parent_id.map(str::to_string),
        format: format.to_string(),
        content: content.to_string(),
        created_at: now,
    })
}

pub async fn list_messages(
    db: &Database,
    thread_id: &str,
) -> HomarrResult<Vec<AssistantMessageRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "assistant_message" WHERE "thread_id" = ? ORDER BY "created_at" ASC"#,
            )
            .bind(thread_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(AssistantMessageRecord {
                        id: r.try_get("id")?,
                        thread_id: r.try_get("thread_id")?,
                        parent_id: r.try_get("parent_id")?,
                        format: r.try_get("format")?,
                        content: r.try_get("content")?,
                        created_at: r.try_get("created_at")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "assistant_message" WHERE "thread_id" = $1 ORDER BY "created_at" ASC"#,
            )
            .bind(thread_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let c: chrono::NaiveDateTime = r.try_get("created_at")?;
                    Ok(AssistantMessageRecord {
                        id: r.try_get("id")?,
                        thread_id: r.try_get("thread_id")?,
                        parent_id: r.try_get("parent_id")?,
                        format: r.try_get("format")?,
                        content: r.try_get("content")?,
                        created_at: naive_to_secs(c),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn delete_thread(db: &Database, thread_id: &str, user_id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"DELETE FROM "assistant_thread" WHERE "id" = ? AND "user_id" = ?"#)
                .bind(thread_id)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"DELETE FROM "assistant_thread" WHERE "id" = $1 AND "user_id" = $2"#)
                .bind(thread_id)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}
