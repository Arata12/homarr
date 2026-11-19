use std::collections::HashMap;

use chrono::Utc;
use homarr_common::{create_id, HomarrError, HomarrResult};
use sqlx::Row;

use crate::models::{
    GroupPermissionEntry, IntegrationRecord, IntegrationSecretRecord, UserPermissionEntry,
};
use crate::pool::map_sqlx_err;
use crate::repos::{naive_to_secs, secs_to_naive};
use crate::Database;

pub struct IntegrationInput {
    pub id: Option<String>,
    pub name: String,
    pub url: String,
    pub external_url: Option<String>,
    pub kind: String,
    pub app_id: Option<String>,
}

pub async fn create(
    db: &Database,
    input: IntegrationInput,
    encrypted_secrets: &[(String, String)],
) -> HomarrResult<IntegrationRecord> {
    let id = input.id.unwrap_or_else(create_id);
    let now = Utc::now().timestamp();

    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "integration" ("id", "name", "url", "external_url", "kind", "app_id")
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.url)
            .bind(&input.external_url)
            .bind(&input.kind)
            .bind(&input.app_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            for (kind, value) in encrypted_secrets {
                sqlx::query(
                    r#"INSERT INTO "integrationSecret" ("kind", "value", "updated_at", "integration_id")
                       VALUES (?, ?, ?, ?)"#,
                )
                .bind(kind)
                .bind(value)
                .bind(now)
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "integration" ("id", "name", "url", "external_url", "kind", "app_id")
                   VALUES ($1, $2, $3, $4, $5, $6)"#,
            )
            .bind(&id)
            .bind(&input.name)
            .bind(&input.url)
            .bind(&input.external_url)
            .bind(&input.kind)
            .bind(&input.app_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            let now_dt = secs_to_naive(now);
            for (kind, value) in encrypted_secrets {
                sqlx::query(
                    r#"INSERT INTO "integrationSecret" ("kind", "value", "updated_at", "integration_id")
                       VALUES ($1, $2, $3, $4)"#,
                )
                .bind(kind)
                .bind(value)
                .bind(now_dt)
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    Ok(IntegrationRecord {
        id,
        name: input.name,
        url: input.url,
        external_url: input.external_url,
        kind: input.kind,
        app_id: input.app_id,
        secret_kinds: encrypted_secrets.iter().map(|(k, _)| k.clone()).collect(),
    })
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<IntegrationRecord>> {
    let mut integrations = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "integration" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(IntegrationRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        url: r.try_get("url")?,
                        external_url: r.try_get("external_url")?,
                        kind: r.try_get("kind")?,
                        app_id: r.try_get("app_id")?,
                        secret_kinds: Vec::new(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "integration" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(IntegrationRecord {
                        id: r.try_get("id")?,
                        name: r.try_get("name")?,
                        url: r.try_get("url")?,
                        external_url: r.try_get("external_url")?,
                        kind: r.try_get("kind")?,
                        app_id: r.try_get("app_id")?,
                        secret_kinds: Vec::new(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
    };

    let mut kind_map: HashMap<String, Vec<String>> = HashMap::new();
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT "integration_id", "kind" FROM "integrationSecret""#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            for r in rows {
                let iid: String = r.try_get("integration_id").map_err(map_sqlx_err)?;
                let kind: String = r.try_get("kind").map_err(map_sqlx_err)?;
                kind_map.entry(iid).or_default().push(kind);
            }
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT "integration_id", "kind" FROM "integrationSecret""#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            for r in rows {
                let iid: String = r.try_get("integration_id").map_err(map_sqlx_err)?;
                let kind: String = r.try_get("kind").map_err(map_sqlx_err)?;
                kind_map.entry(iid).or_default().push(kind);
            }
        }
    }

    for item in &mut integrations {
        if let Some(kinds) = kind_map.remove(&item.id) {
            item.secret_kinds = kinds;
        }
    }
    Ok(integrations)
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<IntegrationRecord>> {
    let record = match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "integration" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(IntegrationRecord {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    url: r.try_get("url")?,
                    external_url: r.try_get("external_url")?,
                    kind: r.try_get("kind")?,
                    app_id: r.try_get("app_id")?,
                    secret_kinds: Vec::new(),
                })
            })
            .transpose()
            .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "integration" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(IntegrationRecord {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    url: r.try_get("url")?,
                    external_url: r.try_get("external_url")?,
                    kind: r.try_get("kind")?,
                    app_id: r.try_get("app_id")?,
                    secret_kinds: Vec::new(),
                })
            })
            .transpose()
            .map_err(map_sqlx_err)?
        }
    };

    let Some(mut rec) = record else {
        return Ok(None);
    };
    let secrets = get_secrets(db, id).await?;
    rec.secret_kinds = secrets.into_iter().map(|s| s.kind).collect();
    Ok(Some(rec))
}

pub async fn get_secrets(
    db: &Database,
    integration_id: &str,
) -> HomarrResult<Vec<IntegrationSecretRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows =
                sqlx::query(r#"SELECT * FROM "integrationSecret" WHERE "integration_id" = ?"#)
                    .bind(integration_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(IntegrationSecretRecord {
                        kind: r.try_get("kind")?,
                        value: r.try_get("value")?,
                        updated_at: r.try_get("updated_at")?,
                        integration_id: r.try_get("integration_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows =
                sqlx::query(r#"SELECT * FROM "integrationSecret" WHERE "integration_id" = $1"#)
                    .bind(integration_id)
                    .fetch_all(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let dt: chrono::NaiveDateTime = r.try_get("updated_at")?;
                    Ok(IntegrationSecretRecord {
                        kind: r.try_get("kind")?,
                        value: r.try_get("value")?,
                        updated_at: naive_to_secs(dt),
                        integration_id: r.try_get("integration_id")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn update(
    db: &Database,
    id: &str,
    name: &str,
    url: &str,
    external_url: Option<&str>,
    app_id: Option<&str>,
    replace_secrets: Option<&[(String, String)]>,
) -> HomarrResult<IntegrationRecord> {
    let now = Utc::now().timestamp();
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            let res = sqlx::query(
                r#"UPDATE "integration" SET "name" = ?, "url" = ?, "external_url" = ?, "app_id" = ? WHERE "id" = ?"#,
            )
            .bind(name)
            .bind(url)
            .bind(external_url)
            .bind(app_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            if res.rows_affected() == 0 {
                return Err(HomarrError::NotFound(format!("Integration {id} not found")));
            }
            if let Some(secrets) = replace_secrets {
                for (kind, value) in secrets {
                    sqlx::query(
                        r#"INSERT INTO "integrationSecret" ("kind", "value", "updated_at", "integration_id")
                           VALUES (?, ?, ?, ?)
                           ON CONFLICT ("integration_id", "kind") DO UPDATE SET "value" = excluded."value", "updated_at" = excluded."updated_at""#,
                    )
                    .bind(kind)
                    .bind(value)
                    .bind(now)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            let res = sqlx::query(
                r#"UPDATE "integration" SET "name" = $1, "url" = $2, "external_url" = $3, "app_id" = $4 WHERE "id" = $5"#,
            )
            .bind(name)
            .bind(url)
            .bind(external_url)
            .bind(app_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            if res.rows_affected() == 0 {
                return Err(HomarrError::NotFound(format!("Integration {id} not found")));
            }
            if let Some(secrets) = replace_secrets {
                let now_dt = secs_to_naive(now);
                for (kind, value) in secrets {
                    sqlx::query(
                        r#"INSERT INTO "integrationSecret" ("kind", "value", "updated_at", "integration_id")
                           VALUES ($1, $2, $3, $4)
                           ON CONFLICT ("integration_id", "kind") DO UPDATE SET "value" = excluded."value", "updated_at" = excluded."updated_at""#,
                    )
                    .bind(kind)
                    .bind(value)
                    .bind(now_dt)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    get_by_id(db, id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Integration not found".to_string()))
}

pub async fn get_permissions(
    db: &Database,
    integration_id: &str,
) -> HomarrResult<(Vec<UserPermissionEntry>, Vec<GroupPermissionEntry>)> {
    match db {
        Database::Sqlite(pool) => {
            let urows = sqlx::query(
                r#"SELECT "user_id", "permission" FROM "integrationUserPermission" WHERE "integration_id" = ?"#,
            )
            .bind(integration_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let grows = sqlx::query(
                r#"SELECT "group_id", "permission" FROM "integrationGroupPermissions" WHERE "integration_id" = ?"#,
            )
            .bind(integration_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;

            let users = urows
                .iter()
                .map(|r| {
                    Ok(UserPermissionEntry {
                        user_id: r.try_get("user_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            let groups = grows
                .iter()
                .map(|r| {
                    Ok(GroupPermissionEntry {
                        group_id: r.try_get("group_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((users, groups))
        }
        Database::Postgres(pool) => {
            let urows = sqlx::query(
                r#"SELECT "user_id", "permission" FROM "integrationUserPermission" WHERE "integration_id" = $1"#,
            )
            .bind(integration_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let grows = sqlx::query(
                r#"SELECT "group_id", "permission" FROM "integrationGroupPermissions" WHERE "integration_id" = $1"#,
            )
            .bind(integration_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;

            let users = urows
                .iter()
                .map(|r| {
                    Ok(UserPermissionEntry {
                        user_id: r.try_get("user_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            let groups = grows
                .iter()
                .map(|r| {
                    Ok(GroupPermissionEntry {
                        group_id: r.try_get("group_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((users, groups))
        }
    }
}

pub async fn set_permissions(
    db: &Database,
    integration_id: &str,
    users: &[UserPermissionEntry],
    groups: &[GroupPermissionEntry],
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "integrationUserPermission" WHERE "integration_id" = ?"#)
                .bind(integration_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "integrationGroupPermissions" WHERE "integration_id" = ?"#)
                .bind(integration_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for u in users {
                sqlx::query(
                    r#"INSERT INTO "integrationUserPermission" ("integration_id", "user_id", "permission") VALUES (?, ?, ?)"#,
                )
                .bind(integration_id)
                .bind(&u.user_id)
                .bind(&u.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for g in groups {
                sqlx::query(
                    r#"INSERT INTO "integrationGroupPermissions" ("integration_id", "group_id", "permission") VALUES (?, ?, ?)"#,
                )
                .bind(integration_id)
                .bind(&g.group_id)
                .bind(&g.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "integrationUserPermission" WHERE "integration_id" = $1"#)
                .bind(integration_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "integrationGroupPermissions" WHERE "integration_id" = $1"#)
                .bind(integration_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for u in users {
                sqlx::query(
                    r#"INSERT INTO "integrationUserPermission" ("integration_id", "user_id", "permission") VALUES ($1, $2, $3)"#,
                )
                .bind(integration_id)
                .bind(&u.user_id)
                .bind(&u.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for g in groups {
                sqlx::query(
                    r#"INSERT INTO "integrationGroupPermissions" ("integration_id", "group_id", "permission") VALUES ($1, $2, $3)"#,
                )
                .bind(integration_id)
                .bind(&g.group_id)
                .bind(&g.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn has_any_delegated_full_access(
    db: &Database,
    user_id: &str,
    group_ids: &[String],
) -> HomarrResult<bool> {
    let user_has: i64 = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"SELECT count(*) as cnt FROM "integrationUserPermission" WHERE "user_id" = ? AND "permission" = 'full'"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_err)?
        .try_get("cnt")
        .map_err(map_sqlx_err)?,
        Database::Postgres(pool) => sqlx::query(
            r#"SELECT count(*) as cnt FROM "integrationUserPermission" WHERE "user_id" = $1 AND "permission" = 'full'"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_err)?
        .try_get("cnt")
        .map_err(map_sqlx_err)?,
    };
    if user_has > 0 {
        return Ok(true);
    }
    for gid in group_ids {
        let cnt: i64 = match db {
            Database::Sqlite(pool) => sqlx::query(
                r#"SELECT count(*) as cnt FROM "integrationGroupPermissions" WHERE "group_id" = ? AND "permission" = 'full'"#,
            )
            .bind(gid)
            .fetch_one(pool)
            .await
            .map_err(map_sqlx_err)?
            .try_get("cnt")
            .map_err(map_sqlx_err)?,
            Database::Postgres(pool) => sqlx::query(
                r#"SELECT count(*) as cnt FROM "integrationGroupPermissions" WHERE "group_id" = $1 AND "permission" = 'full'"#,
            )
            .bind(gid)
            .fetch_one(pool)
            .await
            .map_err(map_sqlx_err)?
            .try_get("cnt")
            .map_err(map_sqlx_err)?,
        };
        if cnt > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "integration" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "integration" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
