use homarr_common::HomarrResult;
use sqlx::Row;

use crate::models::SessionWithUser;
use crate::pool::map_sqlx_err;
use crate::repos::{ms_to_naive, naive_to_ms};
use crate::Database;

pub async fn create(
    db: &Database,
    session_token: &str,
    user_id: &str,
    expires_ms: i64,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "session" ("session_token", "user_id", "expires") VALUES (?, ?, ?)"#,
            )
            .bind(session_token)
            .bind(user_id)
            .bind(expires_ms)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "session" ("session_token", "user_id", "expires") VALUES ($1, $2, $3)"#,
            )
            .bind(session_token)
            .bind(user_id)
            .bind(ms_to_naive(expires_ms))
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn find_with_user(db: &Database, token: &str) -> HomarrResult<Option<SessionWithUser>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(
                r#"SELECT s."session_token", s."user_id", s."expires", u."name", u."email", u."image", u."color_scheme"
                   FROM "session" s
                   INNER JOIN "user" u ON u."id" = s."user_id"
                   WHERE s."session_token" = ?"#,
            )
            .bind(token)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?;

            let Some(row) = row else {
                return Ok(None);
            };
            Ok(Some(SessionWithUser {
                session_token: row.try_get("session_token").map_err(map_sqlx_err)?,
                user_id: row.try_get("user_id").map_err(map_sqlx_err)?,
                expires_ms: row.try_get("expires").map_err(map_sqlx_err)?,
                user_name: row.try_get("name").map_err(map_sqlx_err)?,
                user_email: row.try_get("email").map_err(map_sqlx_err)?,
                user_image: row.try_get("image").map_err(map_sqlx_err)?,
                user_color_scheme: row.try_get("color_scheme").map_err(map_sqlx_err)?,
            }))
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(
                r#"SELECT s."session_token", s."user_id", s."expires", u."name", u."email", u."image", u."color_scheme"
                   FROM "session" s
                   INNER JOIN "user" u ON u."id" = s."user_id"
                   WHERE s."session_token" = $1"#,
            )
            .bind(token)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?;

            let Some(row) = row else {
                return Ok(None);
            };
            let expires: chrono::NaiveDateTime = row.try_get("expires").map_err(map_sqlx_err)?;
            Ok(Some(SessionWithUser {
                session_token: row.try_get("session_token").map_err(map_sqlx_err)?,
                user_id: row.try_get("user_id").map_err(map_sqlx_err)?,
                expires_ms: naive_to_ms(expires),
                user_name: row.try_get("name").map_err(map_sqlx_err)?,
                user_email: row.try_get("email").map_err(map_sqlx_err)?,
                user_image: row.try_get("image").map_err(map_sqlx_err)?,
                user_color_scheme: row.try_get("color_scheme").map_err(map_sqlx_err)?,
            }))
        }
    }
}

pub async fn delete_by_token(db: &Database, token: &str) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"DELETE FROM "session" WHERE "session_token" = ?"#)
                .bind(token)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"DELETE FROM "session" WHERE "session_token" = $1"#)
                .bind(token)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn delete_expired(db: &Database, now_ms: i64) -> HomarrResult<u64> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "session" WHERE "expires" <= ?"#)
            .bind(now_ms)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "session" WHERE "expires" <= $1"#)
            .bind(ms_to_naive(now_ms))
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected)
}

pub async fn delete_by_user_id(db: &Database, user_id: &str) -> HomarrResult<u64> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "session" WHERE "user_id" = ?"#)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "session" WHERE "user_id" = $1"#)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected)
}
