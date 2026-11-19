use homarr_common::{create_id, generate_secure_random_token, HomarrResult};
use sqlx::Row;

use crate::models::InviteRecord;
use crate::pool::map_sqlx_err;
use crate::repos::{naive_to_secs, secs_to_naive};
use crate::Database;

pub async fn create(
    db: &Database,
    creator_id: &str,
    expiration_secs: i64,
) -> HomarrResult<InviteRecord> {
    let id = create_id();
    let token = generate_secure_random_token(24);
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "invite" ("id", "token", "expiration_date", "creator_id") VALUES (?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&token)
            .bind(expiration_secs)
            .bind(creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "invite" ("id", "token", "expiration_date", "creator_id") VALUES ($1, $2, $3, $4)"#,
            )
            .bind(&id)
            .bind(&token)
            .bind(secs_to_naive(expiration_secs))
            .bind(creator_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(InviteRecord {
        id,
        token,
        expiration_date: expiration_secs,
        creator_id: creator_id.to_string(),
    })
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<InviteRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "invite" ORDER BY "expiration_date" DESC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(InviteRecord {
                        id: r.try_get("id")?,
                        token: r.try_get("token")?,
                        expiration_date: r.try_get("expiration_date")?,
                        creator_id: r.try_get("creator_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "invite" ORDER BY "expiration_date" DESC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let dt: chrono::NaiveDateTime = r.try_get("expiration_date")?;
                    Ok(InviteRecord {
                        id: r.try_get("id")?,
                        token: r.try_get("token")?,
                        expiration_date: naive_to_secs(dt),
                        creator_id: r.try_get("creator_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn find_by_token(db: &Database, token: &str) -> HomarrResult<Option<InviteRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "invite" WHERE "token" = ?"#)
                .bind(token)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(InviteRecord {
                    id: r.try_get("id")?,
                    token: r.try_get("token")?,
                    expiration_date: r.try_get("expiration_date")?,
                    creator_id: r.try_get("creator_id")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "invite" WHERE "token" = $1"#)
                .bind(token)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                let dt: chrono::NaiveDateTime = r.try_get("expiration_date")?;
                Ok(InviteRecord {
                    id: r.try_get("id")?,
                    token: r.try_get("token")?,
                    expiration_date: naive_to_secs(dt),
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
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "invite" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "invite" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
