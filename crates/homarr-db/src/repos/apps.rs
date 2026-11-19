use homarr_common::{create_id, HomarrError, HomarrResult};
use sqlx::Row;

use crate::models::AppRecord;
use crate::pool::map_sqlx_err;
use crate::Database;

fn map_app(row: &sqlx::sqlite::SqliteRow) -> Result<AppRecord, sqlx::Error> {
    Ok(AppRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        icon_url: row.try_get("icon_url")?,
        href: row.try_get("href")?,
        ping_url: row.try_get("ping_url")?,
    })
}

pub struct AppInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: String,
    pub href: Option<String>,
    pub ping_url: Option<String>,
}

pub async fn create(db: &Database, input: AppInput) -> HomarrResult<AppRecord> {
    let id = input.id.unwrap_or_else(create_id);
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "app" ("id", "name", "description", "icon_url", "href", "ping_url")
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.description)
            .bind(&input.icon_url)
            .bind(&input.href)
            .bind(&input.ping_url)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "app" ("id", "name", "description", "icon_url", "href", "ping_url")
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.description)
            .bind(&input.icon_url)
            .bind(&input.href)
            .bind(&input.ping_url)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(AppRecord {
        id,
        name: input.name,
        description: input.description,
        icon_url: input.icon_url,
        href: input.href,
        ping_url: input.ping_url,
    })
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<AppRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "app" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref().map(map_app).transpose().map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "app" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(AppRecord {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    description: r.try_get("description")?,
                    icon_url: r.try_get("icon_url")?,
                    href: r.try_get("href")?,
                    ping_url: r.try_get("ping_url")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
    }
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<AppRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "app" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_app)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "app" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(AppRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        description: r.try_get("description")?,
                        icon_url: r.try_get("icon_url")?,
                        href: r.try_get("href")?,
                        ping_url: r.try_get("ping_url")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn list_paginated(
    db: &Database,
    search: Option<&str>,
    page: i64,
    page_size: i64,
) -> HomarrResult<(Vec<AppRecord>, i64)> {
    let offset = (page.max(1) - 1) * page_size.clamp(1, 200);
    let limit = page_size.clamp(1, 200);
    let q = search.unwrap_or("").trim().to_lowercase();
    let pattern = format!("%{q}%");

    match db {
        Database::Sqlite(pool) => {
            let total: i64 = if q.is_empty() {
                sqlx::query(r#"SELECT count(*) as cnt FROM "app""#)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT count(*) as cnt FROM "app" WHERE lower("name") LIKE ? OR lower(coalesce("description", '')) LIKE ?"#)
                    .bind(&pattern)
                    .bind(&pattern)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            };
            let rows = if q.is_empty() {
                sqlx::query(r#"SELECT * FROM "app" ORDER BY lower("name") ASC LIMIT ? OFFSET ?"#)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT * FROM "app" WHERE lower("name") LIKE ? OR lower(coalesce("description", '')) LIKE ? ORDER BY lower("name") ASC LIMIT ? OFFSET ?"#)
                    .bind(&pattern)
                    .bind(&pattern)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            };
            let items = rows
                .iter()
                .map(map_app)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((items, total))
        }
        Database::Postgres(pool) => {
            let total: i64 = if q.is_empty() {
                sqlx::query(r#"SELECT count(*) as cnt FROM "app""#)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT count(*) as cnt FROM "app" WHERE lower("name") LIKE $1 OR lower(coalesce("description", '')) LIKE $2"#)
                    .bind(&pattern)
                    .bind(&pattern)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            };
            let rows = if q.is_empty() {
                sqlx::query(r#"SELECT * FROM "app" ORDER BY lower("name") ASC LIMIT $1 OFFSET $2"#)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT * FROM "app" WHERE lower("name") LIKE $1 OR lower(coalesce("description", '')) LIKE $2 ORDER BY lower("name") ASC LIMIT $3 OFFSET $4"#)
                    .bind(&pattern)
                    .bind(&pattern)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            };
            let items = rows
                .iter()
                .map(|r| {
                    Ok(AppRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        description: r.try_get("description")?,
                        icon_url: r.try_get("icon_url")?,
                        href: r.try_get("href")?,
                        ping_url: r.try_get("ping_url")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((items, total))
        }
    }
}

pub async fn update(db: &Database, id: &str, input: AppInput) -> HomarrResult<AppRecord> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"UPDATE "app" SET "name" = ?, "description" = ?, "icon_url" = ?, "href" = ?, "ping_url" = ? WHERE "id" = ?"#,
        )
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.icon_url)
        .bind(&input.href)
        .bind(&input.ping_url)
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
        Database::Postgres(pool) => sqlx::query(
            r#"UPDATE "app" SET "name" = $1, "description" = $2, "icon_url" = $3, "href" = $4, "ping_url" = $5 WHERE "id" = $6"#,
        )
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.icon_url)
        .bind(&input.href)
        .bind(&input.ping_url)
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
    };
    if affected == 0 {
        return Err(HomarrError::NotFound(format!("App {id} not found")));
    }
    Ok(AppRecord {
        id: id.to_string(),
        name: input.name,
        description: input.description,
        icon_url: input.icon_url,
        href: input.href,
        ping_url: input.ping_url,
    })
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "app" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "app" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
