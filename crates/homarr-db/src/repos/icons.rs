use homarr_common::{create_id, HomarrResult};
use sqlx::Row;

use crate::models::{IconRecord, IconRepositoryRecord};
use crate::pool::map_sqlx_err;
use crate::Database;

pub async fn list_repositories(db: &Database) -> HomarrResult<Vec<IconRepositoryRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "iconRepository" ORDER BY "slug" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(IconRepositoryRecord {
                        id: r.try_get("id")?,
                        slug: r.try_get("slug")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "iconRepository" ORDER BY "slug" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(IconRepositoryRecord {
                        id: r.try_get("id")?,
                        slug: r.try_get("slug")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn search_icons(
    db: &Database,
    query: &str,
    limit: i64,
    offset: i64,
) -> HomarrResult<(Vec<IconRecord>, i64)> {
    let q = query.trim().to_lowercase();
    let pattern = format!("%{q}%");
    match db {
        Database::Sqlite(pool) => {
            let total: i64 = if q.is_empty() {
                sqlx::query(r#"SELECT count(*) as cnt FROM "icon""#)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT count(*) as cnt FROM "icon" WHERE lower("name") LIKE ?"#)
                    .bind(&pattern)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            };
            let rows = if q.is_empty() {
                sqlx::query(r#"SELECT * FROM "icon" ORDER BY "name" ASC LIMIT ? OFFSET ?"#)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(
                    r#"SELECT * FROM "icon" WHERE lower("name") LIKE ? ORDER BY "name" ASC LIMIT ? OFFSET ?"#,
                )
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?
            };
            let icons = rows
                .iter()
                .map(|r| {
                    Ok(IconRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        url: r.try_get("url")?,
                        checksum: r.try_get("checksum")?,
                        icon_repository_id: r.try_get("icon_repository_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((icons, total))
        }
        Database::Postgres(pool) => {
            let total: i64 = if q.is_empty() {
                sqlx::query(r#"SELECT count(*) as cnt FROM "icon""#)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(r#"SELECT count(*) as cnt FROM "icon" WHERE lower("name") LIKE $1"#)
                    .bind(&pattern)
                    .fetch_one(pool)
                    .await
                    .map_err(map_sqlx_err)?
                    .try_get("cnt")
                    .map_err(map_sqlx_err)?
            };
            let rows = if q.is_empty() {
                sqlx::query(r#"SELECT * FROM "icon" ORDER BY "name" ASC LIMIT $1 OFFSET $2"#)
                    .bind(limit)
                    .bind(offset)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?
            } else {
                sqlx::query(
                    r#"SELECT * FROM "icon" WHERE lower("name") LIKE $1 ORDER BY "name" ASC LIMIT $2 OFFSET $3"#,
                )
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?
            };
            let icons = rows
                .iter()
                .map(|r| {
                    Ok(IconRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        url: r.try_get("url")?,
                        checksum: r.try_get("checksum")?,
                        icon_repository_id: r.try_get("icon_repository_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((icons, total))
        }
    }
}

pub struct NewIconEntry {
    pub name: String,
    pub url: String,
    pub checksum: String,
}

pub async fn sync_repository_icons(
    db: &Database,
    slug: &str,
    entries: &[NewIconEntry],
) -> HomarrResult<(usize, usize)> {
    let existing = list_repositories(db).await?;
    let repo_id = if let Some(found) = existing.into_iter().find(|r| r.slug == slug) {
        found.id
    } else {
        let id = create_id();
        match db {
            Database::Sqlite(pool) => {
                sqlx::query(r#"INSERT INTO "iconRepository" ("id", "slug") VALUES (?, ?)"#)
                    .bind(&id)
                    .bind(slug)
                    .execute(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            }
            Database::Postgres(pool) => {
                sqlx::query(r#"INSERT INTO "iconRepository" ("id", "slug") VALUES ($1, $2)"#)
                    .bind(&id)
                    .bind(slug)
                    .execute(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            }
        }
        id
    };

    let deleted: usize = match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"DELETE FROM "icon" WHERE "icon_repository_id" = ?"#)
                .bind(&repo_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected() as usize
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"DELETE FROM "icon" WHERE "icon_repository_id" = $1"#)
                .bind(&repo_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected() as usize
        }
    };

    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for entry in entries {
                let id = create_id();
                sqlx::query(
                    r#"INSERT INTO "icon" ("id", "name", "url", "checksum", "icon_repository_id") VALUES (?, ?, ?, ?, ?)"#,
                )
                .bind(&id)
                .bind(&entry.name)
                .bind(&entry.url)
                .bind(&entry.checksum)
                .bind(&repo_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for entry in entries {
                let id = create_id();
                sqlx::query(
                    r#"INSERT INTO "icon" ("id", "name", "url", "checksum", "icon_repository_id") VALUES ($1, $2, $3, $4, $5)"#,
                )
                .bind(&id)
                .bind(&entry.name)
                .bind(&entry.url)
                .bind(&entry.checksum)
                .bind(&repo_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    Ok((entries.len(), deleted))
}
