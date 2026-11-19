use homarr_common::{HomarrError, HomarrResult};

const BCRYPT_COST: u32 = 10;

pub async fn hash_password(password: &str) -> HomarrResult<String> {
    let pw = password.to_string();
    tokio::task::spawn_blocking(move || bcrypt::hash(pw, BCRYPT_COST))
        .await
        .map_err(|err| HomarrError::Internal(format!("Password hashing task panicked: {err}")))?
        .map_err(|err| HomarrError::Internal(format!("Failed to hash password: {err}")))
}

pub async fn verify_password(password: &str, hash: &str) -> HomarrResult<bool> {
    let pw = password.to_string();
    let h = hash.to_string();
    tokio::task::spawn_blocking(move || bcrypt::verify(pw, &h))
        .await
        .map_err(|err| HomarrError::Internal(format!("Password verify task panicked: {err}")))?
        .map_err(|err| HomarrError::Internal(format!("Failed to verify password: {err}")))
}
