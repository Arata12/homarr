use std::collections::HashMap;

use chrono::Utc;
use homarr_common::HomarrResult;
use sqlx::Row;

use crate::pool::map_sqlx_err;
use crate::repos::secs_to_naive;
use crate::Database;

pub async fn get_user_item_states(
    db: &Database,
    user_id: &str,
    item_id: &str,
) -> HomarrResult<HashMap<String, String>> {
    let rows: Vec<(String, String)> = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT "key", "value" FROM "user_item_state" WHERE "user_id" = ? AND "item_id" = ?"#,
            )
            .bind(user_id)
            .bind(item_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("key")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT "key", "value" FROM "user_item_state" WHERE "user_id" = $1 AND "item_id" = $2"#,
            )
            .bind(user_id)
            .bind(item_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("key")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
    };
    Ok(rows.into_iter().collect())
}

pub async fn set_user_item_state(
    db: &Database,
    user_id: &str,
    item_id: &str,
    key: &str,
    value: &str,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "user_item_state" ("user_id", "item_id", "key", "value")
                   VALUES (?, ?, ?, ?)
                   ON CONFLICT ("user_id", "item_id", "key") DO UPDATE SET "value" = excluded."value""#,
            )
            .bind(user_id)
            .bind(item_id)
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "user_item_state" ("user_id", "item_id", "key", "value")
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT ("user_id", "item_id", "key") DO UPDATE SET "value" = excluded."value""#,
            )
            .bind(user_id)
            .bind(item_id)
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn get_widget_secrets(
    db: &Database,
    item_id: &str,
) -> HomarrResult<HashMap<String, String>> {
    let rows: Vec<(String, String)> = match db {
        Database::Sqlite(pool) => {
            let rows =
                sqlx::query(r#"SELECT "kind", "value" FROM "widget_secret" WHERE "item_id" = ?"#)
                    .bind(item_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("kind")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows =
                sqlx::query(r#"SELECT "kind", "value" FROM "widget_secret" WHERE "item_id" = $1"#)
                    .bind(item_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("kind")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
    };
    Ok(rows.into_iter().collect())
}

pub async fn upsert_widget_secret(
    db: &Database,
    item_id: &str,
    kind: &str,
    encrypted_value: &str,
) -> HomarrResult<()> {
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "widget_secret" ("item_id", "kind", "value", "updated_at")
                   VALUES (?, ?, ?, ?)
                   ON CONFLICT ("item_id", "kind") DO UPDATE SET "value" = excluded."value", "updated_at" = excluded."updated_at""#,
            )
            .bind(item_id)
            .bind(kind)
            .bind(encrypted_value)
            .bind(now)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "widget_secret" ("item_id", "kind", "value", "updated_at")
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT ("item_id", "kind") DO UPDATE SET "value" = excluded."value", "updated_at" = excluded."updated_at""#,
            )
            .bind(item_id)
            .bind(kind)
            .bind(encrypted_value)
            .bind(secs_to_naive(now))
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}
