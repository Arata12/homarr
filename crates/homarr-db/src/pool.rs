use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use homarr_common::{HomarrError, HomarrResult};
use homarr_core::DatabaseDriver;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{PgPool, SqlitePool};

// Tracked migrations (sqlx `_sqlx_migrations` table). See migrations/README.md.
static SQLITE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/sqlite");
static POSTGRES_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");

#[derive(Clone)]
pub enum Database {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(_) => f.write_str("Database::Sqlite(..)"),
            Self::Postgres(_) => f.write_str("Database::Postgres(..)"),
        }
    }
}

impl Database {
    pub async fn connect(driver: DatabaseDriver, url: &str) -> HomarrResult<Self> {
        match driver {
            DatabaseDriver::Sqlite => {
                let conn_str = normalize_sqlite_url(url);
                if conn_str != "sqlite::memory:" {
                    if let Some(file_path) = conn_str.strip_prefix("sqlite://") {
                        let path = Path::new(file_path);
                        if let Some(parent) = path.parent() {
                            if !parent.as_os_str().is_empty() {
                                std::fs::create_dir_all(parent).map_err(|err| {
                                    HomarrError::Database(format!(
                                        "Failed to create SQLite parent directory {}: {err}",
                                        parent.display()
                                    ))
                                })?;
                            }
                        }
                    }
                }
                let opts = SqliteConnectOptions::from_str(&conn_str)
                    .map_err(|err| HomarrError::Database(format!("Invalid SQLite URL: {err}")))?
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .foreign_keys(true)
                    .busy_timeout(Duration::from_secs(5));

                let pool = SqlitePoolOptions::new()
                    .max_connections(if conn_str == "sqlite::memory:" { 1 } else { 5 })
                    .connect_with(opts)
                    .await
                    .map_err(map_sqlx_err)?;
                Ok(Self::Sqlite(pool))
            }
            DatabaseDriver::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(20)
                    .acquire_timeout(Duration::from_secs(10))
                    .connect(url)
                    .await
                    .map_err(map_sqlx_err)?;
                Ok(Self::Postgres(pool))
            }
        }
    }

    pub async fn migrate(&self) -> HomarrResult<()> {
        self.ensure_not_legacy_database().await?;
        match self {
            Self::Sqlite(pool) => SQLITE_MIGRATOR.run(pool).await.map_err(map_migrate_err)?,
            Self::Postgres(pool) => POSTGRES_MIGRATOR.run(pool).await.map_err(map_migrate_err)?,
        }
        Ok(())
    }

    /// Homarr v1 (Next.js/Drizzle) databases have a different schema; running the v3 schema with
    /// `IF NOT EXISTS` over them would silently leave mismatched tables, so refuse to start.
    async fn ensure_not_legacy_database(&self) -> HomarrResult<()> {
        let legacy_tables: i64 = match self {
            Self::Sqlite(pool) => sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '__drizzle_migrations'",
            )
            .fetch_one(pool)
            .await
            .map_err(map_sqlx_err)?,
            Self::Postgres(pool) => sqlx::query_scalar(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = '__drizzle_migrations'",
            )
            .fetch_one(pool)
            .await
            .map_err(map_sqlx_err)?,
        };
        if legacy_tables > 0 {
            return Err(HomarrError::Database(
                "This database was created by Homarr v1 (Drizzle migrations found) and is not                  compatible with Homarr v3. Point DB_URL at a new database."
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub async fn health_check(&self) -> HomarrResult<f64> {
        let start = Instant::now();
        match self {
            Self::Sqlite(pool) => {
                sqlx::query("SELECT 1")
                    .execute(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            }
            Self::Postgres(pool) => {
                sqlx::query("SELECT 1")
                    .execute(pool)
                    .await
                    .map_err(map_sqlx_err)?;
            }
        }
        Ok(start.elapsed().as_secs_f64() * 1000.0)
    }
}

fn normalize_sqlite_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed == ":memory:" || trimmed == "sqlite::memory:" {
        "sqlite::memory:".to_string()
    } else if trimmed.starts_with("sqlite:") || trimmed.starts_with("file:") {
        trimmed.to_string()
    } else {
        format!("sqlite://{trimmed}")
    }
}

fn map_migrate_err(err: sqlx::migrate::MigrateError) -> HomarrError {
    HomarrError::Database(format!("Migration failed: {err}"))
}

pub fn map_sqlx_err(err: sqlx::Error) -> HomarrError {
    if let sqlx::Error::Database(ref db_err) = err {
        let msg = db_err.message();
        let code = db_err.code().unwrap_or_default();
        if code == "23505"
            || code == "2067"
            || code == "1555"
            || msg.contains("UNIQUE constraint failed")
            || msg.contains("duplicate key value")
        {
            return HomarrError::Conflict(msg.to_string());
        }
    }
    if matches!(err, sqlx::Error::RowNotFound) {
        return HomarrError::NotFound("Record not found".to_string());
    }
    HomarrError::Database(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_refuses_homarr_v1_database() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        let Database::Sqlite(pool) = &db else {
            unreachable!()
        };
        sqlx::query("CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT)")
            .execute(pool)
            .await
            .unwrap();
        let err = db.migrate().await.unwrap_err();
        assert!(err.to_string().contains("Homarr v1"), "{err}");
    }
}
