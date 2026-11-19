use chrono::Utc;
use homarr_common::{create_id, HomarrResult};
use sqlx::Row;

use crate::models::{MediaRecord, MediaSummary};
use crate::pool::map_sqlx_err;
use crate::repos::{naive_to_secs, secs_to_naive};
use crate::Database;

pub async fn create(
    db: &Database,
    name: &str,
    content: &[u8],
    content_type: &str,
    creator_id: Option<&str>,
) -> HomarrResult<MediaSummary> {
    let id = create_id();
    let size = content.len() as i32;
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "media" ("id", "name", "content", "content_type", "size", "created_at", "creator_id")
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(content)
            .bind(content_type)
            .bind(size)
            .bind(now)
            .bind(creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "media" ("id", "name", "content", "content_type", "size", "created_at", "creator_id")
                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(content)
            .bind(content_type)
            .bind(size)
            .bind(secs_to_naive(now))
            .bind(creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    Ok(MediaSummary {
        id,
        name: name.to_string(),
        content_type: content_type.to_string(),
        size,
        created_at: now,
        creator_id: creator_id.map(str::to_string),
    })
}

pub async fn list_metadata(db: &Database) -> HomarrResult<Vec<MediaSummary>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT "id", "name", "content_type", "size", "created_at", "creator_id" FROM "media" ORDER BY "created_at" DESC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(MediaSummary {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        content_type: r.try_get("content_type")?,
                        size: r.try_get("size")?,
                        created_at: r.try_get("created_at")?,
                        creator_id: r.try_get("creator_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT "id", "name", "content_type", "size", "created_at", "creator_id" FROM "media" ORDER BY "created_at" DESC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let dt: chrono::NaiveDateTime = r.try_get("created_at")?;
                    Ok(MediaSummary {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        content_type: r.try_get("content_type")?,
                        size: r.try_get("size")?,
                        created_at: naive_to_secs(dt),
                        creator_id: r.try_get("creator_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<MediaRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "media" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(MediaRecord {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    content: r.try_get("content")?,
                    content_type: r.try_get("content_type")?,
                    size: r.try_get("size")?,
                    created_at: r.try_get("created_at")?,
                    creator_id: r.try_get("creator_id")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "media" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                let dt: chrono::NaiveDateTime = r.try_get("created_at")?;
                Ok(MediaRecord {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    content: r.try_get("content")?,
                    content_type: r.try_get("content_type")?,
                    size: r.try_get("size")?,
                    created_at: naive_to_secs(dt),
                    creator_id: r.try_get("creator_id")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
    }
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "media" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "media" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
