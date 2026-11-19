use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_definitions::default_header_preferences_serialized;
use sqlx::{postgres::PgRow, sqlite::SqliteRow, Row};

use crate::models::UserRecord;
use crate::pool::map_sqlx_err;
use crate::repos::{ms_to_naive, naive_to_ms};
use crate::Database;

fn map_sqlite_user(row: &SqliteRow) -> Result<UserRecord, sqlx::Error> {
    Ok(UserRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        email: row.try_get("email")?,
        email_verified: row.try_get("email_verified")?,
        image: row.try_get("image")?,
        password: row.try_get("password")?,
        provider: row.try_get("provider")?,
        home_board_id: row.try_get("home_board_id")?,
        mobile_home_board_id: row.try_get("mobile_home_board_id")?,
        default_search_engine_id: row.try_get("default_search_engine_id")?,
        open_search_in_new_tab: row.try_get("open_search_in_new_tab")?,
        ddg_bangs: row.try_get("ddg_bangs")?,
        color_scheme: row.try_get("color_scheme")?,
        byte_unit_system: row.try_get("byte_unit_system")?,
        first_day_of_week: row.try_get("first_day_of_week")?,
        ping_icons_enabled: row.try_get("ping_icons_enabled")?,
        enable_right_click_on_widgets: row.try_get("enable_right_click_on_widgets")?,
        header_preferences: row.try_get("header_preferences")?,
        completed_manage_tour: row.try_get("completed_manage_tour")?,
        completed_board_tour: row.try_get("completed_board_tour")?,
    })
}

fn map_pg_user(row: &PgRow) -> Result<UserRecord, sqlx::Error> {
    let email_verified: Option<chrono::NaiveDateTime> = row.try_get("email_verified")?;
    let first_day: i16 = row.try_get("first_day_of_week")?;
    Ok(UserRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        email: row.try_get("email")?,
        email_verified: email_verified.map(naive_to_ms),
        image: row.try_get("image")?,
        password: row.try_get("password")?,
        provider: row.try_get("provider")?,
        home_board_id: row.try_get("home_board_id")?,
        mobile_home_board_id: row.try_get("mobile_home_board_id")?,
        default_search_engine_id: row.try_get("default_search_engine_id")?,
        open_search_in_new_tab: row.try_get("open_search_in_new_tab")?,
        ddg_bangs: row.try_get("ddg_bangs")?,
        color_scheme: row.try_get("color_scheme")?,
        byte_unit_system: row.try_get("byte_unit_system")?,
        first_day_of_week: i32::from(first_day),
        ping_icons_enabled: row.try_get("ping_icons_enabled")?,
        enable_right_click_on_widgets: row.try_get("enable_right_click_on_widgets")?,
        header_preferences: row.try_get("header_preferences")?,
        completed_manage_tour: row.try_get("completed_manage_tour")?,
        completed_board_tour: row.try_get("completed_board_tour")?,
    })
}

pub struct CreateUserInput {
    pub id: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub provider: String,
    pub image: Option<String>,
}

pub async fn create(db: &Database, input: CreateUserInput) -> HomarrResult<UserRecord> {
    let id = input.id.unwrap_or_else(create_id);
    let header_prefs = default_header_preferences_serialized();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "user" ("id", "name", "email", "password", "provider", "image", "header_preferences")
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.email)
            .bind(&input.password_hash)
            .bind(&input.provider)
            .bind(&input.image)
            .bind(&header_prefs)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "user" ("id", "name", "email", "password", "provider", "image", "header_preferences")
                   VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.email)
            .bind(&input.password_hash)
            .bind(&input.provider)
            .bind(&input.image)
            .bind(&header_prefs)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    find_by_id(db, &id)
        .await?
        .ok_or_else(|| HomarrError::Internal("Inserted user missing".to_string()))
}

pub async fn find_by_id(db: &Database, id: &str) -> HomarrResult<Option<UserRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "user" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_user)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "user" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_user)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn find_by_name_ci(db: &Database, username: &str) -> HomarrResult<Option<UserRecord>> {
    let lower = username.trim().to_lowercase();
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "user" WHERE lower("name") = ?"#)
                .bind(&lower)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_user)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "user" WHERE lower("name") = $1"#)
                .bind(&lower)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_user)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<UserRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "user" ORDER BY "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_user)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "user" ORDER BY "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_user)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn search(db: &Database, query: &str, limit: i64) -> HomarrResult<Vec<UserRecord>> {
    let pattern = format!("%{}%", query.trim().to_lowercase());
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "user" WHERE lower(coalesce("name", '')) LIKE ? OR lower(coalesce("email", '')) LIKE ? ORDER BY "name" ASC LIMIT ?"#,
            )
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_user)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "user" WHERE lower(coalesce("name", '')) LIKE $1 OR lower(coalesce("email", '')) LIKE $2 ORDER BY "name" ASC LIMIT $3"#,
            )
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_user)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn count(db: &Database) -> HomarrResult<i64> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT count(*) as cnt FROM "user""#)
                .fetch_one(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.try_get("cnt").map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT count(*) as cnt FROM "user""#)
                .fetch_one(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.try_get("cnt").map_err(map_sqlx_err)
        }
    }
}

pub async fn update_password(
    db: &Database,
    user_id: &str,
    password_hash: &str,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"UPDATE "user" SET "password" = ? WHERE "id" = ?"#)
                .bind(password_hash)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"UPDATE "user" SET "password" = $1 WHERE "id" = $2"#)
                .bind(password_hash)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn update_profile(
    db: &Database,
    user_id: &str,
    name: Option<&str>,
    email: Option<&str>,
    image: Option<&str>,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"UPDATE "user" SET "name" = coalesce(?, "name"), "email" = ?, "image" = coalesce(?, "image") WHERE "id" = ?"#)
                .bind(name)
                .bind(email)
                .bind(image)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"UPDATE "user" SET "name" = coalesce($1, "name"), "email" = $2, "image" = coalesce($3, "image") WHERE "id" = $4"#)
                .bind(name)
                .bind(email)
                .bind(image)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct UpdateUserPreferencesInput {
    pub home_board_id: Option<Option<String>>,
    pub mobile_home_board_id: Option<Option<String>>,
    pub default_search_engine_id: Option<Option<String>>,
    pub open_search_in_new_tab: Option<bool>,
    pub ddg_bangs: Option<bool>,
    pub color_scheme: Option<String>,
    pub byte_unit_system: Option<String>,
    pub first_day_of_week: Option<i32>,
    pub ping_icons_enabled: Option<bool>,
    pub enable_right_click_on_widgets: Option<bool>,
    pub header_preferences: Option<String>,
    pub completed_manage_tour: Option<bool>,
    pub completed_board_tour: Option<bool>,
}

pub async fn update_preferences(
    db: &Database,
    user_id: &str,
    patch: UpdateUserPreferencesInput,
) -> HomarrResult<UserRecord> {
    let current = find_by_id(db, user_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("User {user_id} not found")))?;

    let home_board_id = patch.home_board_id.unwrap_or(current.home_board_id);
    let mobile_home_board_id = patch
        .mobile_home_board_id
        .unwrap_or(current.mobile_home_board_id);
    let default_search_engine_id = patch
        .default_search_engine_id
        .unwrap_or(current.default_search_engine_id);
    let open_search_in_new_tab = patch
        .open_search_in_new_tab
        .unwrap_or(current.open_search_in_new_tab);
    let ddg_bangs = patch.ddg_bangs.unwrap_or(current.ddg_bangs);
    let color_scheme = patch.color_scheme.unwrap_or(current.color_scheme);
    let byte_unit_system = patch.byte_unit_system.unwrap_or(current.byte_unit_system);
    let first_day_of_week = patch.first_day_of_week.unwrap_or(current.first_day_of_week);
    let ping_icons_enabled = patch
        .ping_icons_enabled
        .unwrap_or(current.ping_icons_enabled);
    let enable_right_click_on_widgets = patch
        .enable_right_click_on_widgets
        .unwrap_or(current.enable_right_click_on_widgets);
    let header_preferences = patch
        .header_preferences
        .unwrap_or(current.header_preferences);
    let completed_manage_tour = patch
        .completed_manage_tour
        .unwrap_or(current.completed_manage_tour);
    let completed_board_tour = patch
        .completed_board_tour
        .unwrap_or(current.completed_board_tour);

    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"UPDATE "user" SET
                   "home_board_id" = ?, "mobile_home_board_id" = ?, "default_search_engine_id" = ?,
                   "open_search_in_new_tab" = ?, "ddg_bangs" = ?, "color_scheme" = ?,
                   "byte_unit_system" = ?, "first_day_of_week" = ?, "ping_icons_enabled" = ?,
                   "enable_right_click_on_widgets" = ?, "header_preferences" = ?,
                   "completed_manage_tour" = ?, "completed_board_tour" = ?
                   WHERE "id" = ?"#,
            )
            .bind(&home_board_id)
            .bind(&mobile_home_board_id)
            .bind(&default_search_engine_id)
            .bind(open_search_in_new_tab)
            .bind(ddg_bangs)
            .bind(&color_scheme)
            .bind(&byte_unit_system)
            .bind(first_day_of_week)
            .bind(ping_icons_enabled)
            .bind(enable_right_click_on_widgets)
            .bind(&header_preferences)
            .bind(completed_manage_tour)
            .bind(completed_board_tour)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"UPDATE "user" SET
                   "home_board_id" = $1, "mobile_home_board_id" = $2, "default_search_engine_id" = $3,
                   "open_search_in_new_tab" = $4, "ddg_bangs" = $5, "color_scheme" = $6,
                   "byte_unit_system" = $7, "first_day_of_week" = $8, "ping_icons_enabled" = $9,
                   "enable_right_click_on_widgets" = $10, "header_preferences" = $11,
                   "completed_manage_tour" = $12, "completed_board_tour" = $13
                   WHERE "id" = $14"#,
            )
            .bind(&home_board_id)
            .bind(&mobile_home_board_id)
            .bind(&default_search_engine_id)
            .bind(open_search_in_new_tab)
            .bind(ddg_bangs)
            .bind(&color_scheme)
            .bind(&byte_unit_system)
            .bind(first_day_of_week as i16)
            .bind(ping_icons_enabled)
            .bind(enable_right_click_on_widgets)
            .bind(&header_preferences)
            .bind(completed_manage_tour)
            .bind(completed_board_tour)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    find_by_id(db, user_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("User not found".to_string()))
}

pub async fn mark_email_verified(db: &Database, user_id: &str, ms: i64) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"UPDATE "user" SET "email_verified" = ? WHERE "id" = ?"#)
                .bind(ms)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"UPDATE "user" SET "email_verified" = $1 WHERE "id" = $2"#)
                .bind(ms_to_naive(ms))
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn delete(db: &Database, user_id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "user" WHERE "id" = ?"#)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "user" WHERE "id" = $1"#)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}

pub async fn fix_usernames(db: &Database) -> HomarrResult<usize> {
    let users = list_all(db).await?;
    let mut updated = 0usize;
    for user in users {
        if let Some(name) = user.name {
            let lower = name.trim().to_lowercase();
            if lower != name {
                match db {
                    Database::Sqlite(pool) => {
                        sqlx::query(r#"UPDATE "user" SET "name" = ? WHERE "id" = ?"#)
                            .bind(&lower)
                            .bind(&user.id)
                            .execute(pool)
                            .await
                            .map_err(map_sqlx_err)?;
                    }
                    Database::Postgres(pool) => {
                        sqlx::query(r#"UPDATE "user" SET "name" = $1 WHERE "id" = $2"#)
                            .bind(&lower)
                            .bind(&user.id)
                            .execute(pool)
                            .await
                            .map_err(map_sqlx_err)?;
                    }
                }
                updated += 1;
            }
        }
    }
    Ok(updated)
}
