use std::collections::HashMap;

use homarr_common::{create_id, deserialize_superjson, serialize_superjson, HomarrResult};
use homarr_definitions::settings::default_server_setting_json;
use serde_json::Value;
use sqlx::Row;

use crate::models::{CronJobConfigRecord, OnboardingRecord, TrustedCertificateRecord};
use crate::pool::map_sqlx_err;
use crate::Database;

pub async fn get_setting_raw(db: &Database, key: &str) -> HomarrResult<Option<String>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT "value" FROM "serverSetting" WHERE "setting_key" = ?"#)
                .bind(key)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| r.try_get("value"))
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row =
                sqlx::query(r#"SELECT "value" FROM "serverSetting" WHERE "setting_key" = $1"#)
                    .bind(key)
                    .fetch_optional(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            row.map(|r| r.try_get("value"))
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_setting(db: &Database, key: &str) -> HomarrResult<Value> {
    if let Some(raw) = get_setting_raw(db, key).await? {
        if let Ok(val) = deserialize_superjson::<Value>(&raw) {
            return Ok(val);
        }
    }
    Ok(default_server_setting_json(key).unwrap_or_else(|| Value::Object(Default::default())))
}

pub async fn get_all_server_settings(db: &Database) -> HomarrResult<HashMap<String, Value>> {
    let rows: Vec<(String, String)> = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT "setting_key", "value" FROM "serverSetting""#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("setting_key")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT "setting_key", "value" FROM "serverSetting""#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("setting_key")?, r.try_get("value")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
    };

    let mut map = HashMap::new();
    for key in homarr_definitions::settings::DEFAULT_SERVER_SETTINGS_KEYS {
        if let Some(def) = default_server_setting_json(key) {
            map.insert(key.to_string(), def);
        }
    }
    for (k, raw) in rows {
        if let Ok(val) = deserialize_superjson::<Value>(&raw) {
            map.insert(k, val);
        }
    }
    Ok(map)
}

pub async fn set_setting(db: &Database, key: &str, value: &Value) -> HomarrResult<()> {
    let encoded = serialize_superjson(value)?;
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "serverSetting" ("setting_key", "value") VALUES (?, ?)
                   ON CONFLICT ("setting_key") DO UPDATE SET "value" = excluded."value""#,
            )
            .bind(key)
            .bind(&encoded)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "serverSetting" ("setting_key", "value") VALUES ($1, $2)
                   ON CONFLICT ("setting_key") DO UPDATE SET "value" = excluded."value""#,
            )
            .bind(key)
            .bind(&encoded)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn get_onboarding(db: &Database) -> HomarrResult<Option<OnboardingRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "onboarding" LIMIT 1"#)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(OnboardingRecord {
                    id: r.try_get("id")?,
                    step: r.try_get("step")?,
                    previous_step: r.try_get("previous_step")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "onboarding" LIMIT 1"#)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.map(|r| {
                Ok(OnboardingRecord {
                    id: r.try_get("id")?,
                    step: r.try_get("step")?,
                    previous_step: r.try_get("previous_step")?,
                })
            })
            .transpose()
            .map_err(map_sqlx_err)
        }
    }
}

pub async fn set_onboarding(
    db: &Database,
    step: &str,
    previous_step: Option<&str>,
) -> HomarrResult<OnboardingRecord> {
    if let Some(current) = get_onboarding(db).await? {
        match db {
            Database::Sqlite(pool) => {
                sqlx::query(
                    r#"UPDATE "onboarding" SET "step" = ?, "previous_step" = ? WHERE "id" = ?"#,
                )
                .bind(step)
                .bind(previous_step)
                .bind(&current.id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
            }
            Database::Postgres(pool) => {
                sqlx::query(
                    r#"UPDATE "onboarding" SET "step" = $1, "previous_step" = $2 WHERE "id" = $3"#,
                )
                .bind(step)
                .bind(previous_step)
                .bind(&current.id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?;
            }
        }
        return Ok(OnboardingRecord {
            id: current.id,
            step: step.to_string(),
            previous_step: previous_step.map(str::to_string),
        });
    }

    let id = create_id();
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "onboarding" ("id", "step", "previous_step") VALUES (?, ?, ?)"#,
            )
            .bind(&id)
            .bind(step)
            .bind(previous_step)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "onboarding" ("id", "step", "previous_step") VALUES ($1, $2, $3)"#,
            )
            .bind(&id)
            .bind(step)
            .bind(previous_step)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(OnboardingRecord {
        id,
        step: step.to_string(),
        previous_step: previous_step.map(str::to_string),
    })
}

pub async fn list_cron_configs(db: &Database) -> HomarrResult<Vec<CronJobConfigRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "cron_job_configuration" ORDER BY "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(CronJobConfigRecord {
                        name: r.try_get("name")?,
                        cron_expression: r.try_get("cron_expression")?,
                        is_enabled: r.try_get("is_enabled")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "cron_job_configuration" ORDER BY "name" ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(CronJobConfigRecord {
                        name: r.try_get("name")?,
                        cron_expression: r.try_get("cron_expression")?,
                        is_enabled: r.try_get("is_enabled")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn upsert_cron_config(
    db: &Database,
    name: &str,
    cron_expression: &str,
    is_enabled: bool,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "cron_job_configuration" ("name", "cron_expression", "is_enabled")
                   VALUES (?, ?, ?)
                   ON CONFLICT ("name") DO UPDATE SET "cron_expression" = excluded."cron_expression", "is_enabled" = excluded."is_enabled""#,
            )
            .bind(name)
            .bind(cron_expression)
            .bind(is_enabled)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "cron_job_configuration" ("name", "cron_expression", "is_enabled")
                   VALUES ($1, $2, $3)
                   ON CONFLICT ("name") DO UPDATE SET "cron_expression" = excluded."cron_expression", "is_enabled" = excluded."is_enabled""#,
            )
            .bind(name)
            .bind(cron_expression)
            .bind(is_enabled)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn list_trusted_hostnames(db: &Database) -> HomarrResult<Vec<TrustedCertificateRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "trusted_certificate_hostname" ORDER BY "hostname" ASC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(TrustedCertificateRecord {
                        hostname: r.try_get("hostname")?,
                        thumbprint: r.try_get("thumbprint")?,
                        certificate: r.try_get("certificate")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "trusted_certificate_hostname" ORDER BY "hostname" ASC"#,
            )
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(TrustedCertificateRecord {
                        hostname: r.try_get("hostname")?,
                        thumbprint: r.try_get("thumbprint")?,
                        certificate: r.try_get("certificate")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn upsert_trusted_hostname(
    db: &Database,
    hostname: &str,
    thumbprint: &str,
    certificate: &str,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "trusted_certificate_hostname" ("hostname", "thumbprint", "certificate")
                   VALUES (?, ?, ?)
                   ON CONFLICT ("hostname", "thumbprint") DO UPDATE SET "certificate" = excluded."certificate""#,
            )
            .bind(hostname)
            .bind(thumbprint)
            .bind(certificate)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "trusted_certificate_hostname" ("hostname", "thumbprint", "certificate")
                   VALUES ($1, $2, $3)
                   ON CONFLICT ("hostname", "thumbprint") DO UPDATE SET "certificate" = excluded."certificate""#,
            )
            .bind(hostname)
            .bind(thumbprint)
            .bind(certificate)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn delete_trusted_hostname(
    db: &Database,
    hostname: &str,
    thumbprint: &str,
) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"DELETE FROM "trusted_certificate_hostname" WHERE "hostname" = ? AND "thumbprint" = ?"#,
        )
        .bind(hostname)
        .bind(thumbprint)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
        Database::Postgres(pool) => sqlx::query(
            r#"DELETE FROM "trusted_certificate_hostname" WHERE "hostname" = $1 AND "thumbprint" = $2"#,
        )
        .bind(hostname)
        .bind(thumbprint)
        .execute(pool)
        .await
        .map_err(map_sqlx_err)?
        .rows_affected(),
    };
    Ok(affected > 0)
}
