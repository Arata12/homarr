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

    #[tokio::test]
    async fn sqlite_migrate_is_tracked_and_idempotent() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        db.migrate().await.unwrap();
        let Database::Sqlite(pool) = &db else {
            unreachable!()
        };
        let rows: Vec<(i64, bool)> =
            sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![(1, true)]);
    }

    #[tokio::test]
    async fn sqlite_legacy_untracked_database_upgrades() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        let Database::Sqlite(pool) = &db else {
            unreachable!()
        };
        // Old startup behaviour: raw 0001 applied, no tracking table.
        sqlx::raw_sql(include_str!("../../../migrations/sqlite/0001_initial.sql"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO \"user\" (id, name) VALUES ('u1', 'legacy')")
            .execute(pool)
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM \"user\"")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(users, 1);
    }

    #[tokio::test]
    async fn save_board_content_preserves_collapse_and_item_state() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        let crypto = SecretCrypto::zero_for_test();
        seed::seed_demo(&db, &crypto).await.unwrap();

        let demo = repos::users::find_by_name_ci(&db, "demo")
            .await
            .unwrap()
            .unwrap();
        let board = repos::boards::get_full_board_by_name(&db, "Home")
            .await
            .unwrap()
            .unwrap();
        let section_id = board.sections[0].id.clone();
        let item_id = board.items[0].id.clone();

        repos::boards::set_section_collapse(&db, &demo.id, &section_id, true)
            .await
            .unwrap();
        let Database::Sqlite(pool) = &db else {
            unreachable!()
        };
        sqlx::query(
            r#"INSERT INTO "user_item_state" ("user_id", "item_id", "key", "value") VALUES (?, ?, 'k', 'v')"#,
        )
        .bind(&demo.id)
        .bind(&item_id)
        .execute(pool)
        .await
        .unwrap();

        let sections_in: Vec<repos::boards::SaveBoardSectionInput> = board
            .sections
            .iter()
            .map(|s| repos::boards::SaveBoardSectionInput {
                id: s.id.clone(),
                kind: s.kind.clone(),
                x_offset: s.x_offset,
                y_offset: s.y_offset,
                name: s.name.clone(),
                options: s.options.clone(),
                layouts: s.layouts.clone(),
            })
            .collect();
        let items_in: Vec<repos::boards::SaveBoardItemInput> = board
            .items
            .iter()
            .map(|i| repos::boards::SaveBoardItemInput {
                id: i.id.clone(),
                kind: i.kind.clone(),
                options: i.options.clone(),
                advanced_options: i.advanced_options.clone(),
                layouts: i.layouts.clone(),
                integration_ids: i.integration_ids.clone(),
            })
            .collect();
        repos::boards::save_board_content(&db, &board.board.id, &sections_in, &items_in)
            .await
            .unwrap();

        let collapsed = repos::boards::get_section_collapse_states(&db, &demo.id)
            .await
            .unwrap();
        assert_eq!(collapsed.get(&section_id), Some(&true));
        let (state_count,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM "user_item_state" WHERE "user_id" = ? AND "item_id" = ?"#,
        )
        .bind(&demo.id)
        .bind(&item_id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(state_count, 1);
    }

    #[tokio::test]
    async fn starter_board_seeds_cleanly() {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        seed::seed_defaults(&db).await.unwrap();
        let user = repos::users::create(
            &db,
            repos::users::CreateUserInput {
                id: None,
                name: "admin".into(),
                email: None,
                password_hash: Some("hash".into()),
                provider: "credentials".into(),
                image: None,
            },
        )
        .await
        .unwrap();
        let board_id = seed::seed_starter_board(&db, &user.id, "dashboard", 10)
            .await
            .unwrap();
        let full = repos::boards::get_full_board_by_id(&db, &board_id)
            .await
            .unwrap()
            .expect("starter board must exist");
        assert_eq!(full.board.name, "dashboard");
        assert_eq!(full.items.len(), 7);
        assert_eq!(repos::apps::list_all(&db).await.unwrap().len(), 4);
    }
}
