use chrono::Utc;
use homarr_common::{generate_secure_random_token, HomarrResult};
use homarr_db::models::ApiKeyRecord;
use homarr_db::{repos, Database};

use crate::password::{hash_password, verify_password};
use crate::permissions::AuthSession;

pub const API_KEY_HEADER_NAME: &str = "ApiKey";

#[derive(Debug, Clone)]
pub struct OAuthKeyMeta {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
}

pub async fn create_api_key(
    db: &Database,
    user_id: &str,
    read_only: bool,
    oauth: Option<OAuthKeyMeta>,
) -> HomarrResult<(String, ApiKeyRecord)> {
    let secret = generate_secure_random_token(32);
    let hashed_secret = hash_password(&secret).await?;
    let record = repos::api_keys::create(
        db,
        repos::api_keys::CreateApiKeyInput {
            user_id: user_id.to_string(),
            hashed_secret,
            read_only,
            oauth_client_id: oauth.as_ref().map(|o| o.client_id.clone()),
            oauth_client_name: oauth.as_ref().map(|o| o.client_name.clone()),
            oauth_redirect_uri: oauth.as_ref().map(|o| o.redirect_uri.clone()),
            oauth_issued_at_ms: oauth.as_ref().map(|_| Utc::now().timestamp_millis()),
        },
    )
    .await?;

    let plaintext = format!("{}.{}", record.id, secret);
    Ok((plaintext, record))
}

pub async fn authenticate_api_key(
    db: &Database,
    header_value: &str,
) -> HomarrResult<Option<AuthSession>> {
    let parts: Vec<&str> = header_value.trim().split('.').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        tracing::warn!(
            reason = "API_KEY_INVALID_FORMAT",
            "Failed to authenticate with api-key"
        );
        return Ok(None);
    }

    let api_key_id = parts[0];
    let api_key_secret = parts[1];

    let Some(found) = repos::api_keys::find_by_id_with_user(db, api_key_id).await? else {
        tracing::warn!(
            reason = "API_KEY_NOT_FOUND",
            "Failed to authenticate with api-key"
        );
        return Ok(None);
    };

    let valid = verify_password(api_key_secret, &found.api_key.api_key).await?;
    if !valid {
        tracing::warn!(
            reason = "API_KEY_MISMATCH",
            "Failed to authenticate with api-key"
        );
        return Ok(None);
    }

    let user_id = found.api_key.user_id.clone();
    let permissions = repos::groups::get_user_permissions(db, &user_id).await?;
    let group_ids = repos::groups::get_user_group_ids(db, &user_id).await?;

    Ok(Some(AuthSession::from_user(
        user_id,
        found.user_name,
        found.user_email,
        None,
        found.user_color_scheme,
        permissions,
        group_ids,
        found.api_key.read_only,
        found.api_key.oauth_client_id,
    )))
}
