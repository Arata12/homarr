use rand::RngCore;

/// Generates a new CUID2 identifier compatible with Homarr v2 database IDs.
pub fn create_id() -> String {
    cuid2::create_id()
}

/// Generates a cryptographically secure random hex token representing `byte_len` bytes
/// (producing a string of length `byte_len * 2`).
pub fn generate_secure_random_token(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_unique_cuid2_and_random_tokens() {
        let a = create_id();
        let b = create_id();
        assert!(!a.is_empty());
        assert_ne!(a, b);

        let token = generate_secure_random_token(32);
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
