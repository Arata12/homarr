use homarr_common::{create_id, HomarrError, HomarrResult};
use sqlx::Row;

use crate::models::SearchEngineRecord;
use crate::pool::map_sqlx_err;
use crate::Database;

pub struct SearchEngineInput {
    pub id: Option<String>,
    pub icon_url: String,
    pub name: String,
    pub short: String,
    pub description: Option<String>,
    pub url_template: Option<String>,
    pub kind: String,
    pub integration_id: Option<String>,
}

pub async fn create(db: &Database, input: SearchEngineInput) -> HomarrResult<SearchEngineRecord> {
    let id = input.id.unwrap_or_else(create_id);
    let short = input.short.trim().to_lowercase();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "search_engine" ("id", "icon_url", "name", "short", "description", "url_template", "type", "integration_id")
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.icon_url)
            .bind(&input.name)
            .bind(&short)
            .bind(&input.description)
            .bind(&input.url_template)
            .bind(&input.kind)
            .bind(&input.integration_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "search_engine" ("id", "icon_url", "name", "short", "description", "url_template", "type", "integration_id")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            )
            .bind(&id)
            .bind(&input.icon_url)
            .bind(&input.name)
            .bind(&short)
            .bind(&input.description)
            .bind(&input.url_template)
            .bind(&input.kind)
            .bind(&input.integration_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(SearchEngineRecord {
        id,
        icon_url: input.icon_url,
        name: input.name,
        short,
        description: input.description,
        url_template: input.url_template,
        kind: input.kind,
        integration_id: input.integration_id,
    })
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<SearchEngineRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "search_engine" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(SearchEngineRecord {
                        id: r.try_get("id")?,
                        icon_url: r.try_get("icon_url")?,
                        name: r.try_get("name")?,
                        short: r.try_get("short")?,
                        description: r.try_get("description")?,
                        url_template: r.try_get("url_template")?,
                        kind: r.try_get("type")?,
                        integration_id: r.try_get("integration_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "search_engine" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(SearchEngineRecord {
                        id: r.try_get("id")?,
                        icon_url: r.try_get("icon_url")?,
                        name: r.try_get("name")?,
                        short: r.try_get("short")?,
                        description: r.try_get("description")?,
                        url_template: r.try_get("url_template")?,
                        kind: r.try_get("type")?,
                        integration_id: r.try_get("integration_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<SearchEngineRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "search_engine" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(SearchEngineRecord {
                    id: r.try_get("id")?,
                    icon_url: r.try_get("icon_url")?,
                    name: r.try_get("name")?,
                    short: r.try_get("short")?,
                    description: r.try_get("description")?,
                    url_template: r.try_get("url_template")?,
                    kind: r.try_get("type")?,
                    integration_id: r.try_get("integration_id")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "search_engine" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(SearchEngineRecord {
                    id: r.try_get("id")?,
                    icon_url: r.try_get("icon_url")?,
                    name: r.try_get("name")?,
                    short: r.try_get("short")?,
                    description: r.try_get("description")?,
                    url_template: r.try_get("url_template")?,
                    kind: r.try_get("type")?,
                    integration_id: r.try_get("integration_id")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
    }
}

pub async fn update(
    db: &Database,
    id: &str,
    input: SearchEngineInput,
) -> HomarrResult<SearchEngineRecord> {
    let short = input.short.trim().to_lowercase();
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"UPDATE "search_engine" SET "icon_url" = ?, "name" = ?, "short" = ?, "description" = ?, "url_template" = ?, "type" = ?, "integration_id" = ? WHERE "id" = ?"#,
        )
        .bind(&input.icon_url)
        .bind(&input.name)
        .bind(&short)
        .bind(&input.description)
        .bind(&input.url_template)
        .bind(&input.kind)
        .bind(&input.integration_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
        Database::Postgres(pool) => sqlx::query(
            r#"UPDATE "search_engine" SET "icon_url" = $1, "name" = $2, "short" = $3, "description" = $4, "url_template" = $5, "type" = $6, "integration_id" = $7 WHERE "id" = $8"#,
        )
        .bind(&input.icon_url)
        .bind(&input.name)
        .bind(&short)
        .bind(&input.description)
        .bind(&input.url_template)
        .bind(&input.kind)
        .bind(&input.integration_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
    };
    if affected == 0 {
        return Err(HomarrError::NotFound(format!(
            "Search engine {id} not found"
        )));
    }
    Ok(SearchEngineRecord {
        id: id.to_string(),
        icon_url: input.icon_url,
        name: input.name,
        short,
        description: input.description,
        url_template: input.url_template,
        kind: input.kind,
        integration_id: input.integration_id,
    })
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "search_engine" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "search_engine" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
