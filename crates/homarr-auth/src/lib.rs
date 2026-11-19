pub mod api_key;
pub mod extractors;
pub mod password;
pub mod permissions;
pub mod session;

pub use api_key::{authenticate_api_key, create_api_key, API_KEY_HEADER_NAME};
pub use extractors::{AuthContextProvider, OptionalAuth, RequireAdmin, RequireAuth};
pub use password::{hash_password, verify_password};
pub use permissions::{
    can_view_board, check_board_permission, check_integration_permission, AuthSession,
};
pub use session::{
    build_clear_session_cookie, build_session_cookie, cookie_secure, create_session,
    delete_session, get_session_from_token, session_cookie_name,
};

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_common::SecretCrypto;
    use homarr_core::DatabaseDriver;
    use homarr_db::{repos, seed, Database};
    use homarr_definitions::{BoardPermission, GroupPermissionKey};

    #[tokio::test]
    async fn password_session_api_key_and_permissions_work() {
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
        assert!(verify_password("demo", demo.password.as_deref().unwrap())
            .await
            .unwrap());
        assert!(!verify_password("wrong", demo.password.as_deref().unwrap())
            .await
            .unwrap());

        let token = create_session(&db, &demo.id, 3600).await.unwrap();
        let sess = get_session_from_token(&db, &token).await.unwrap().unwrap();
        assert_eq!(sess.user_id, demo.id);
        assert!(sess.has_permission(GroupPermissionKey::Admin));
        assert!(!sess.api_key_read_only);

        let (plain_key, key_rec) = create_api_key(&db, &demo.id, true, None).await.unwrap();
        let api_sess = authenticate_api_key(&db, &plain_key)
            .await
            .unwrap()
            .expect("api key should authenticate");
        assert_eq!(api_sess.user_id, demo.id);
        assert!(api_sess.api_key_read_only);
        assert!(api_sess.require_writable(false).is_err());

        // Revoked key fails
        assert!(repos::api_keys::delete(&db, &key_rec.id).await.unwrap());
        assert!(authenticate_api_key(&db, &plain_key)
            .await
            .unwrap()
            .is_none());

        // Board permission check on public demo board
        let board = repos::boards::get_by_name(&db, "Home")
            .await
            .unwrap()
            .unwrap();
        assert!(can_view_board(&db, None, &board).await.unwrap());
        assert!(
            check_board_permission(&db, Some(&sess), &board, BoardPermission::Full)
                .await
                .is_ok()
        );

        delete_session(&db, &token).await.unwrap();
        assert!(get_session_from_token(&db, &token).await.unwrap().is_none());
    }
}
