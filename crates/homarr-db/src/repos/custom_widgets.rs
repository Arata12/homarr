use chrono::Utc;
use homarr_common::{create_id, HomarrError, HomarrResult};
use serde_json::Value;
use sqlx::{postgres::PgRow, sqlite::SqliteRow, Row};

use crate::models::{CustomWidgetV2Record, CustomWidgetV2SecretRecord};
use crate::pool::map_sqlx_err;
use crate::repos::{naive_to_secs, secs_to_naive};
use crate::Database;

fn map_sqlite_cw(row: &SqliteRow) -> Result<CustomWidgetV2Record, sqlx::Error> {
    let sources_str: String = row.try_get("sources")?;
    let requests_str: String = row.try_get("requests")?;
    let options_str: String = row.try_get("options")?;
    let workshop_str: Option<String> = row.try_get("workshop")?;
    Ok(CustomWidgetV2Record {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        icon_url: row.try_get("icon_url")?,
        sources: serde_json::from_str(&sources_str).unwrap_or(Value::Array(vec![])),
        requests: serde_json::from_str(&requests_str).unwrap_or(Value::Array(vec![])),
        options: serde_json::from_str(&options_str).unwrap_or(Value::Array(vec![])),
        template: row.try_get("template")?,
        workshop: workshop_str.and_then(|s| serde_json::from_str(&s).ok()),
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        creator_id: row.try_get("creator_id")?,
    })
}

fn map_pg_cw(row: &PgRow) -> Result<CustomWidgetV2Record, sqlx::Error> {
    let sources_str: String = row.try_get("sources")?;
    let requests_str: String = row.try_get("requests")?;
    let options_str: String = row.try_get("options")?;
    let workshop_str: Option<String> = row.try_get("workshop")?;
    let created: chrono::NaiveDateTime = row.try_get("created_at")?;
    let updated: chrono::NaiveDateTime = row.try_get("updated_at")?;
    Ok(CustomWidgetV2Record {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        icon_url: row.try_get("icon_url")?,
        sources: serde_json::from_str(&sources_str).unwrap_or(Value::Array(vec![])),
        requests: serde_json::from_str(&requests_str).unwrap_or(Value::Array(vec![])),
        options: serde_json::from_str(&options_str).unwrap_or(Value::Array(vec![])),
        template: row.try_get("template")?,
        workshop: workshop_str.and_then(|s| serde_json::from_str(&s).ok()),
        enabled: row.try_get("enabled")?,
        created_at: naive_to_secs(created),
        updated_at: naive_to_secs(updated),
        creator_id: row.try_get("creator_id")?,
    })
}

pub struct CustomWidgetV2Input {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub sources: Value,
    pub requests: Value,
    pub options: Value,
    pub template: String,
    pub workshop: Option<Value>,
    pub enabled: bool,
    pub creator_id: Option<String>,
}

pub async fn create_v2(
    db: &Database,
    input: CustomWidgetV2Input,
) -> HomarrResult<CustomWidgetV2Record> {
    let id = input.id.unwrap_or_else(create_id);
    let now = Utc::now().timestamp();
    let sources_str = serde_json::to_string(&input.sources)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let requests_str = serde_json::to_string(&input.requests)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let options_str = serde_json::to_string(&input.options)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let workshop_str = input
        .workshop
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;

    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "custom_widget_v2_definition"
                   ("id", "name", "description", "icon_url", "sources", "requests", "options", "template", "workshop", "enabled", "created_at", "updated_at", "creator_id")
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.description)
            .bind(&input.icon_url)
            .bind(&sources_str)
            .bind(&requests_str)
            .bind(&options_str)
            .bind(&input.template)
            .bind(&workshop_str)
            .bind(input.enabled)
            .bind(now)
            .bind(now)
            .bind(&input.creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let dt = secs_to_naive(now);
            sqlx::query(
                r#"INSERT INTO "custom_widget_v2_definition"
                   ("id", "name", "description", "icon_url", "sources", "requests", "options", "template", "workshop", "enabled", "created_at", "updated_at", "creator_id")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.description)
            .bind(&input.icon_url)
            .bind(&sources_str)
            .bind(&requests_str)
            .bind(&options_str)
            .bind(&input.template)
            .bind(&workshop_str)
            .bind(input.enabled)
            .bind(dt)
            .bind(dt)
            .bind(&input.creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    get_v2_by_id(db, &id)
        .await?
        .ok_or_else(|| HomarrError::Internal("Inserted custom widget missing".to_string()))
}

pub async fn list_v2(db: &Database) -> HomarrResult<Vec<CustomWidgetV2Record>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "custom_widget_v2_definition" ORDER BY "updated_at" DESC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_cw)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "custom_widget_v2_definition" ORDER BY "updated_at" DESC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_cw)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_v2_by_id(db: &Database, id: &str) -> HomarrResult<Option<CustomWidgetV2Record>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "custom_widget_v2_definition" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_cw)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "custom_widget_v2_definition" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_cw)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn update_v2(
    db: &Database,
    id: &str,
    input: CustomWidgetV2Input,
) -> HomarrResult<CustomWidgetV2Record> {
    let now = Utc::now().timestamp();
    let sources_str = serde_json::to_string(&input.sources)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let requests_str = serde_json::to_string(&input.requests)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let options_str = serde_json::to_string(&input.options)
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;
    let workshop_str = input
        .workshop
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| HomarrError::BadRequest(e.to_string()))?;

    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"UPDATE "custom_widget_v2_definition"
               SET "name" = ?, "description" = ?, "icon_url" = ?, "sources" = ?, "requests" = ?, "options" = ?, "template" = ?, "workshop" = ?, "enabled" = ?, "updated_at" = ?
               WHERE "id" = ?"#,
        )
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.icon_url)
        .bind(&sources_str)
        .bind(&requests_str)
        .bind(&options_str)
        .bind(&input.template)
        .bind(&workshop_str)
        .bind(input.enabled)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
        Database::Postgres(pool) => sqlx::query(
            r#"UPDATE "custom_widget_v2_definition"
               SET "name" = $1, "description" = $2, "icon_url" = $3, "sources" = $4, "requests" = $5, "options" = $6, "template" = $7, "workshop" = $8, "enabled" = $9, "updated_at" = $10
               WHERE "id" = $11"#,
        )
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.icon_url)
        .bind(&sources_str)
        .bind(&requests_str)
        .bind(&options_str)
        .bind(&input.template)
        .bind(&workshop_str)
        .bind(input.enabled)
        .bind(secs_to_naive(now))
        .bind(id)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
    };
    if affected == 0 {
        return Err(HomarrError::NotFound(format!(
            "Custom widget {id} not found"
        )));
    }
    get_v2_by_id(db, id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Custom widget not found".to_string()))
}

pub async fn delete_v2(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"DELETE FROM "custom_widget_v2_definition" WHERE "id" = ?"#)
                .bind(id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"DELETE FROM "custom_widget_v2_definition" WHERE "id" = $1"#)
                .bind(id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

pub async fn list_v2_secrets(
    db: &Database,
    definition_id: &str,
) -> HomarrResult<Vec<CustomWidgetV2SecretRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows =
                sqlx::query(r#"SELECT * FROM "custom_widget_v2_secret" WHERE "definition_id" = ?"#)
                    .bind(definition_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(CustomWidgetV2SecretRecord {
                        definition_id: r.try_get("definition_id")?,
                        source_id: r.try_get("source_id")?,
                        kind: r.try_get("kind")?,
                        encrypted_value: r.try_get("encrypted_value")?,
                        updated_at: r.try_get("updated_at")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "custom_widget_v2_secret" WHERE "definition_id" = $1"#,
            )
            .bind(definition_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let dt: chrono::NaiveDateTime = r.try_get("updated_at")?;
                    Ok(CustomWidgetV2SecretRecord {
                        definition_id: r.try_get("definition_id")?,
                        source_id: r.try_get("source_id")?,
                        kind: r.try_get("kind")?,
                        encrypted_value: r.try_get("encrypted_value")?,
                        updated_at: naive_to_secs(dt),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn upsert_v2_secret(
    db: &Database,
    definition_id: &str,
    source_id: &str,
    kind: &str,
    encrypted_value: &str,
) -> HomarrResult<()> {
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "custom_widget_v2_secret" ("source_id", "kind", "encrypted_value", "updated_at", "definition_id")
                   VALUES (?, ?, ?, ?, ?)
                   ON CONFLICT ("definition_id", "source_id", "kind") DO UPDATE SET "encrypted_value" = excluded."encrypted_value", "updated_at" = excluded."updated_at""#,
            )
            .bind(source_id)
            .bind(kind)
            .bind(encrypted_value)
            .bind(now)
            .bind(definition_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "custom_widget_v2_secret" ("source_id", "kind", "encrypted_value", "updated_at", "definition_id")
                   VALUES ($1, $2, $3, $4, $5)
                   ON CONFLICT ("definition_id", "source_id", "kind") DO UPDATE SET "encrypted_value" = excluded."encrypted_value", "updated_at" = excluded."updated_at""#,
            )
            .bind(source_id)
            .bind(kind)
            .bind(encrypted_value)
            .bind(secs_to_naive(now))
            .bind(definition_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}
