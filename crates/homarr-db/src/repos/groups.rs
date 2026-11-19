use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_definitions::{get_permissions_with_children, GroupPermissionKey};
use sqlx::{postgres::PgRow, sqlite::SqliteRow, Row};

use crate::models::{GroupMemberUser, GroupRecord, GroupWithDetails};
use crate::pool::map_sqlx_err;
use crate::Database;

fn map_sqlite_group(row: &SqliteRow) -> Result<GroupRecord, sqlx::Error> {
    Ok(GroupRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        owner_id: row.try_get("owner_id")?,
        home_board_id: row.try_get("home_board_id")?,
        mobile_home_board_id: row.try_get("mobile_home_board_id")?,
        position: row.try_get("position")?,
    })
}

fn map_pg_group(row: &PgRow) -> Result<GroupRecord, sqlx::Error> {
    let pos: i16 = row.try_get("position")?;
    Ok(GroupRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        owner_id: row.try_get("owner_id")?,
        home_board_id: row.try_get("home_board_id")?,
        mobile_home_board_id: row.try_get("mobile_home_board_id")?,
        position: i32::from(pos),
    })
}

pub async fn create(
    db: &Database,
    name: &str,
    owner_id: Option<&str>,
    position: i32,
) -> HomarrResult<GroupRecord> {
    let id = create_id();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "group" ("id", "name", "owner_id", "position") VALUES (?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(owner_id)
            .bind(position)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "group" ("id", "name", "owner_id", "position") VALUES ($1, $2, $3, $4)"#,
            )
            .bind(&id)
            .bind(name)
            .bind(owner_id)
            .bind(position as i16)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    get_by_id(db, &id)
        .await?
        .ok_or_else(|| HomarrError::Internal("Inserted group missing".to_string()))
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<GroupRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "group" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_group)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "group" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_group)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_by_name(db: &Database, name: &str) -> HomarrResult<Option<GroupRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "group" WHERE "name" = ?"#)
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_group)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "group" WHERE "name" = $1"#)
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_group)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<GroupRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "group" ORDER BY "position" ASC, "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_group)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "group" ORDER BY "position" ASC, "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_group)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_with_details(db: &Database, id: &str) -> HomarrResult<Option<GroupWithDetails>> {
    let Some(group) = get_by_id(db, id).await? else {
        return Ok(None);
    };
    let permissions = get_group_permissions(db, id).await?;
    let members = get_group_members(db, id).await?;
    Ok(Some(GroupWithDetails {
        group,
        permissions,
        members,
    }))
}

pub async fn get_group_permissions(db: &Database, group_id: &str) -> HomarrResult<Vec<String>> {
    match db {
        Database::Sqlite(pool) => {
            let rows =
                sqlx::query(r#"SELECT "permission" FROM "groupPermission" WHERE "group_id" = ?"#)
                    .bind(group_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("permission"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows =
                sqlx::query(r#"SELECT "permission" FROM "groupPermission" WHERE "group_id" = $1"#)
                    .bind(group_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("permission"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_group_members(
    db: &Database,
    group_id: &str,
) -> HomarrResult<Vec<GroupMemberUser>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT u."id", u."name", u."email", u."image"
                   FROM "groupMember" gm
                   INNER JOIN "user" u ON u."id" = gm."user_id"
                   WHERE gm."group_id" = ?
                   ORDER BY u."name" ASC"#,
            )
            .bind(group_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(GroupMemberUser {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        email: r.try_get("email")?,
                        image: r.try_get("image")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT u."id", u."name", u."email", u."image"
                   FROM "groupMember" gm
                   INNER JOIN "user" u ON u."id" = gm."user_id"
                   WHERE gm."group_id" = $1
                   ORDER BY u."name" ASC"#,
            )
            .bind(group_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(GroupMemberUser {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        email: r.try_get("email")?,
                        image: r.try_get("image")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn add_member(db: &Database, group_id: &str, user_id: &str) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT OR IGNORE INTO "groupMember" ("group_id", "user_id") VALUES (?, ?)"#,
            )
            .bind(group_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "groupMember" ("group_id", "user_id") VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
            )
            .bind(group_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn remove_member(db: &Database, group_id: &str, user_id: &str) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"DELETE FROM "groupMember" WHERE "group_id" = ? AND "user_id" = ?"#)
                .bind(group_id)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"DELETE FROM "groupMember" WHERE "group_id" = $1 AND "user_id" = $2"#)
                .bind(group_id)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn set_permissions(
    db: &Database,
    group_id: &str,
    permissions: &[String],
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "groupPermission" WHERE "group_id" = ?"#)
                .bind(group_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for perm in permissions {
                sqlx::query(
                    r#"INSERT INTO "groupPermission" ("group_id", "permission") VALUES (?, ?)"#,
                )
                .bind(group_id)
                .bind(perm)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "groupPermission" WHERE "group_id" = $1"#)
                .bind(group_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for perm in permissions {
                sqlx::query(
                    r#"INSERT INTO "groupPermission" ("group_id", "permission") VALUES ($1, $2)"#,
                )
                .bind(group_id)
                .bind(perm)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn get_user_permissions(
    db: &Database,
    user_id: &str,
) -> HomarrResult<Vec<GroupPermissionKey>> {
    let raw_perms: Vec<String> = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT DISTINCT gp."permission"
                   FROM "groupPermission" gp
                   INNER JOIN "groupMember" gm ON gp."group_id" = gm."group_id"
                   WHERE gm."user_id" = ?"#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("permission"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT DISTINCT gp."permission"
                   FROM "groupPermission" gp
                   INNER JOIN "groupMember" gm ON gp."group_id" = gm."group_id"
                   WHERE gm."user_id" = $1"#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("permission"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
    };

    let parsed: Vec<GroupPermissionKey> = raw_perms
        .iter()
        .filter_map(|p| GroupPermissionKey::from_str_opt(p))
        .collect();
    Ok(get_permissions_with_children(&parsed))
}

pub async fn get_user_group_ids(db: &Database, user_id: &str) -> HomarrResult<Vec<String>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT "group_id" FROM "groupMember" WHERE "user_id" = ?"#)
                .bind(user_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("group_id"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT "group_id" FROM "groupMember" WHERE "user_id" = $1"#)
                .bind(user_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| r.try_get("group_id"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn update_settings(
    db: &Database,
    group_id: &str,
    name: Option<&str>,
    home_board_id: Option<Option<&str>>,
    mobile_home_board_id: Option<Option<&str>>,
) -> HomarrResult<GroupRecord> {
    let current = get_by_id(db, group_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Group {group_id} not found")))?;
    let next_name = name.unwrap_or(&current.name).to_string();
    let next_home = home_board_id
        .map(|v| v.map(str::to_string))
        .unwrap_or(current.home_board_id);
    let next_mobile = mobile_home_board_id
        .map(|v| v.map(str::to_string))
        .unwrap_or(current.mobile_home_board_id);

    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"UPDATE "group" SET "name" = ?, "home_board_id" = ?, "mobile_home_board_id" = ? WHERE "id" = ?"#,
            )
            .bind(&next_name)
            .bind(&next_home)
            .bind(&next_mobile)
            .bind(group_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"UPDATE "group" SET "name" = $1, "home_board_id" = $2, "mobile_home_board_id" = $3 WHERE "id" = $4"#,
            )
            .bind(&next_name)
            .bind(&next_home)
            .bind(&next_mobile)
            .bind(group_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    get_by_id(db, group_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Group not found".to_string()))
}

pub async fn delete(db: &Database, group_id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "group" WHERE "id" = ?"#)
            .bind(group_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "group" WHERE "id" = $1"#)
            .bind(group_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
