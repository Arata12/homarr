pub mod backup;
pub mod models;
pub mod pool;
pub mod repos;
pub mod seed;

pub use pool::Database;

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_common::SecretCrypto;
    use homarr_core::DatabaseDriver;
    use homarr_definitions::GroupPermissionKey;

    async fn exercise_database(db: &Database) {
        db.migrate().await.expect("migrations should succeed");
        let latency = db
            .health_check()
            .await
            .expect("health check should succeed");
        assert!(latency >= 0.0);

        let crypto = SecretCrypto::zero_for_test();
        seed::seed_demo(db, &crypto)
            .await
            .expect("demo seed should succeed");

        let demo = repos::users::find_by_name_ci(db, "DEMO")
            .await
            .unwrap()
            .expect("demo user exists");
        let perms = repos::groups::get_user_permissions(db, &demo.id)
            .await
            .unwrap();
        assert!(perms.contains(&GroupPermissionKey::Admin));
        assert!(perms.contains(&GroupPermissionKey::BoardFullAll));

        let board = repos::boards::get_full_board_by_name(db, "Home")
            .await
            .unwrap()
            .expect("Home board exists");
        assert!(!board.items.is_empty());
        assert!(board.layouts.len() >= 2);

        let duplicated =
            repos::boards::duplicate_board(db, &board.board.id, "Home Copy", Some(&demo.id))
                .await
                .unwrap();
        assert_eq!(duplicated.items.len(), board.items.len());
        assert!(repos::boards::delete(db, &duplicated.board.id)
            .await
            .unwrap());

        let bundle = backup::export_backup_json(db).await.unwrap();
        assert!(!bundle.boards.is_empty());
        assert!(!bundle.apps.is_empty());
    }

    #[tokio::test]
    async fn sqlite_migrations_seed_and_crud() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        exercise_database(&db).await;
    }

    #[tokio::test]
    async fn postgres_migrations_seed_and_crud_when_available() {
        let Ok(url) = std::env::var("POSTGRES_URL") else {
            return;
        };
        if url.trim().is_empty() {
            return;
        }
        let db = Database::connect(DatabaseDriver::Postgres, &url)
            .await
            .expect("should connect to compose postgres");
        exercise_database(&db).await;
    }
}
