use aes::Aes256;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::{HomarrError, HomarrResult};

type Aes256CbcEnc = cbc::Encryptor<Aes256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

const FINGERPRINT_DOMAIN: &[u8] = b"homarr-keyed-fingerprint-v1";

/// AES-256-CBC and keyed HMAC helper binary-compatible with Homarr v2 Node implementation.
#[derive(Clone)]
pub struct SecretCrypto {
    key: [u8; 32],
    fingerprint_key: [u8; 32],
}

impl std::fmt::Debug for SecretCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretCrypto")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl SecretCrypto {
    /// Creates a new `SecretCrypto` from a 64-character hex string.
    pub fn new(hex_key: &str) -> HomarrResult<Self> {
        let trimmed = hex_key.trim();
        if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(HomarrError::BadRequest(
                "SECRET_ENCRYPTION_KEY must be a 64-character hexadecimal string".to_string(),
            ));
        }

        let decoded = hex::decode(trimmed).map_err(|err| {
            HomarrError::BadRequest(format!("Invalid SECRET_ENCRYPTION_KEY hex: {err}"))
        })?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded);

        let mut mac = <HmacSha256 as Mac>::new_from_slice(&key)
            .map_err(|_| HomarrError::Internal("Failed to initialize HMAC key".to_string()))?;
        mac.update(FINGERPRINT_DOMAIN);
        let fingerprint_bytes = mac.finalize().into_bytes();
        let mut fingerprint_key = [0u8; 32];
        fingerprint_key.copy_from_slice(&fingerprint_bytes);

        Ok(Self {
            key,
            fingerprint_key,
        })
    }

    /// Creates a zero-key `SecretCrypto` instance solely for testing or fallback when explicitly intended.
    pub fn zero_for_test() -> Self {
        Self::new(&"0".repeat(64)).expect("zero hex key is always valid")
    }

    /// Returns the raw 32-byte encryption key (used by HKDF in ImageProxy).
    pub fn raw_key(&self) -> &[u8; 32] {
        &self.key
    }

    /// Encrypts plaintext using AES-256-CBC with a random 16-byte IV, formatted as `{ciphertext_hex}.{iv_hex}`.
    pub fn encrypt_secret(&self, plaintext: &str) -> HomarrResult<String> {
        let mut iv = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut iv);
        self.encrypt_secret_with_iv(plaintext, &iv)
    }

    fn encrypt_secret_with_iv(&self, plaintext: &str, iv: &[u8; 16]) -> HomarrResult<String> {
        let cipher = Aes256CbcEnc::new_from_slices(&self.key, iv)
            .map_err(|err| HomarrError::Internal(format!("Cipher init failed: {err}")))?;
        let ciphertext = cipher.encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());
        Ok(format!("{}.{}", hex::encode(ciphertext), hex::encode(iv)))
    }

    /// Decrypts a `{ciphertext_hex}.{iv_hex}` secret.
    pub fn decrypt_secret(&self, encrypted: &str) -> HomarrResult<String> {
        let (ciphertext_hex, iv_hex) = encrypted.split_once('.').ok_or_else(|| {
            HomarrError::BadRequest("Invalid encrypted secret format".to_string())
        })?;

        let iv = hex::decode(iv_hex)
            .map_err(|err| HomarrError::BadRequest(format!("Invalid IV hex: {err}")))?;
        if iv.len() != 16 {
            return Err(HomarrError::BadRequest("Invalid IV length".to_string()));
        }

        let ciphertext = hex::decode(ciphertext_hex)
            .map_err(|err| HomarrError::BadRequest(format!("Invalid ciphertext hex: {err}")))?;

        let cipher = Aes256CbcDec::new_from_slices(&self.key, &iv)
            .map_err(|err| HomarrError::Internal(format!("Decipher init failed: {err}")))?;
        let plaintext_bytes = cipher
            .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
            .map_err(|_| HomarrError::BadRequest("Failed to decrypt secret".to_string()))?;

        String::from_utf8(plaintext_bytes)
            .map_err(|_| HomarrError::BadRequest("Decrypted secret is not valid UTF-8".to_string()))
    }

    /// Creates a deterministic keyed HMAC-SHA256 fingerprint hex digest.
    pub fn create_keyed_fingerprint(&self, value: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.fingerprint_key)
            .expect("HMAC accepts any key length");
        mac.update(value.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Constant-time verification of a keyed HMAC-SHA256 fingerprint.
    pub fn verify_keyed_fingerprint(&self, value: &str, fingerprint: &str) -> bool {
        let expected_hex = self.create_keyed_fingerprint(value);
        let Ok(expected) = hex::decode(&expected_hex) else {
            return false;
        };
        let Ok(actual) = hex::decode(fingerprint) else {
            return false;
        };
        if expected.len() != actual.len() {
            return false;
        }
        bool::from(expected.ct_eq(&actual))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let crypto =
            SecretCrypto::new("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .unwrap();
        let secret = "super-secret-api-key-12345!";
        let encrypted = crypto.encrypt_secret(secret).unwrap();
        assert!(encrypted.contains('.'));
        let decrypted = crypto.decrypt_secret(&encrypted).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn node_crypto_known_vector_compatibility() {
        // Key = all zeros (32 bytes), IV = 000102030405060708090a0b0c0d0e0f, plaintext = "hello homarr"
        let crypto = SecretCrypto::zero_for_test();
        let iv: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let encrypted = crypto.encrypt_secret_with_iv("hello homarr", &iv).unwrap();
        assert_eq!(crypto.decrypt_secret(&encrypted).unwrap(), "hello homarr");
        assert!(encrypted.ends_with(".000102030405060708090a0b0c0d0e0f"));
    }

    #[test]
    fn keyed_fingerprint_verification() {
        let crypto = SecretCrypto::zero_for_test();
        let fp = crypto.create_keyed_fingerprint("image-proxy:persistent:abc.def");
        assert_eq!(fp.len(), 64);
        assert!(crypto.verify_keyed_fingerprint("image-proxy:persistent:abc.def", &fp));
        assert!(!crypto.verify_keyed_fingerprint("image-proxy:persistent:tampered.def", &fp));
        assert!(!crypto.verify_keyed_fingerprint("image-proxy:persistent:abc.def", "00"));
    }

    #[test]
    fn rejects_invalid_key() {
        assert!(SecretCrypto::new("short").is_err());
        assert!(SecretCrypto::new(&"z".repeat(64)).is_err());
    }
}
