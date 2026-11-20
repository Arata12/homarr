use std::collections::{BTreeMap, HashMap};

use aes::Aes256;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use homarr_common::{is_http_url, validate_outbound_url, HomarrError, HomarrResult, SecretCrypto};
use homarr_core::HttpClientFactory;
use homarr_redis::RedisStore;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type Aes256CbcEnc = cbc::Encryptor<Aes256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

const HKDF_INFO: &[u8] = b"homarr-image-proxy-v1";
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const FORWARDED_IMAGE_TTL_SECS: u64 = 3600;

#[derive(Debug, Serialize, Deserialize)]
struct StoredImagePayload {
    url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    trusted_origin: Option<String>,
}

#[derive(Debug)]
pub enum ImageProxyResult {
    Redirect(String),
    Bytes { content_type: String, data: Vec<u8> },
}

#[derive(Clone)]
pub struct ImageProxyService {
    key: [u8; 32],
    http: HttpClientFactory,
    redis: RedisStore,
}

impl ImageProxyService {
    pub fn new(crypto: &SecretCrypto, http: HttpClientFactory, redis: RedisStore) -> Self {
        let hk = Hkdf::<Sha256>::new(None, crypto.raw_key());
        let mut derived = [0u8; 32];
        hk.expand(HKDF_INFO, &mut derived)
            .expect("32-byte HKDF expansion is always valid");
        Self {
            key: derived,
            http,
            redis,
        }
    }

    pub fn derived_key(&self) -> &[u8; 32] {
        &self.key
    }

    pub fn create_persistent_image_url(&self, raw_url: &str) -> HomarrResult<String> {
        let trimmed = raw_url.trim();
        if !is_http_url(trimmed) {
            return Err(HomarrError::BadRequest(
                "Persistent image URL must start with http:// or https://".to_string(),
            ));
        }
        let encoded = URL_SAFE_NO_PAD.encode(trimmed.as_bytes());
        let sig = self.sign_token(&encoded)?;
        Ok(format!("/api/image-proxy/persistent.{encoded}.{sig}"))
    }

    pub fn verify_persistent_token(&self, id: &str) -> HomarrResult<Option<String>> {
        let Some(rest) = id.strip_prefix("persistent.") else {
            return Ok(None);
        };
        let Some((encoded, sig_hex)) = rest.split_once('.') else {
            return Err(HomarrError::BadRequest(
                "Malformed persistent image token".to_string(),
            ));
        };
        let expected_sig = self.sign_token(encoded)?;
        let expected_bytes = hex::decode(&expected_sig).unwrap_or_default();
        let supplied_bytes = hex::decode(sig_hex).map_err(|_| {
            HomarrError::Forbidden("Invalid persistent image signature".to_string())
        })?;
        if expected_bytes.len() != supplied_bytes.len()
            || !bool::from(expected_bytes.ct_eq(&supplied_bytes))
        {
            return Err(HomarrError::Forbidden(
                "Invalid persistent image signature".to_string(),
            ));
        }

        let decoded = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).map_err(|err| {
            HomarrError::BadRequest(format!("Invalid persistent URL encoding: {err}"))
        })?;
        let target_url = String::from_utf8(decoded).map_err(|err| {
            HomarrError::BadRequest(format!("Invalid persistent URL UTF-8: {err}"))
        })?;
        if !is_http_url(&target_url) {
            return Err(HomarrError::BadRequest(
                "Persistent URL scheme is not allowed".to_string(),
            ));
        }
        Ok(Some(target_url))
    }

    pub async fn forward_image(
        &self,
        url: &str,
        headers: HashMap<String, String>,
    ) -> HomarrResult<String> {
        self.forward_trusted_image(url, headers, None).await
    }

    pub async fn forward_trusted_image(
        &self,
        url: &str,
        headers: HashMap<String, String>,
        trusted_origin: Option<String>,
    ) -> HomarrResult<String> {
        let _ = validate_outbound_url(url, trusted_origin.as_deref())?;
        // Deterministic id (keyed HMAC of url + headers + origin) so refreshes reuse the same proxy URL.
        let sorted_headers: BTreeMap<&String, &String> = headers.iter().collect();
        let id_input = serde_json::to_string(&(url, &sorted_headers, &trusted_origin))
            .map_err(|err| HomarrError::Internal(format!("Image id encode failed: {err}")))?;
        let id = self.sign_token(&format!("image-id:{id_input}"))?;
        let payload = StoredImagePayload {
            url: url.to_string(),
            headers,
            trusted_origin,
        };
        let json = serde_json::to_string(&payload)
            .map_err(|err| HomarrError::Internal(format!("Image payload encode failed: {err}")))?;
        let encrypted = self.encrypt_payload(&json)?;
        self.redis
            .set_string_ex(&format!("img:{id}"), &encrypted, FORWARDED_IMAGE_TTL_SECS)
            .await?;
        Ok(format!("/api/image-proxy/{id}"))
    }

    pub async fn resolve_image(&self, id: &str) -> HomarrResult<ImageProxyResult> {
        if let Some(target) = self.verify_persistent_token(id)? {
            return Ok(ImageProxyResult::Redirect(target));
        }

        let Some(encrypted) = self.redis.get_string(&format!("img:{id}")).await? else {
            return Err(HomarrError::NotFound(format!(
                "Proxied image '{id}' not found"
            )));
        };
        let decrypted = self.decrypt_payload(&encrypted)?;
        let payload: StoredImagePayload = serde_json::from_str(&decrypted)
            .map_err(|err| HomarrError::Internal(format!("Invalid stored image payload: {err}")))?;

        let (bytes, content_type, status) = self
            .http
            .guarded_get(
                &payload.url,
                Some(&payload.headers),
                payload.trusted_origin.as_deref(),
                MAX_IMAGE_BYTES,
            )
            .await?;
        if status >= 400 {
            return Err(HomarrError::Upstream(format!(
                "Image upstream returned HTTP {status}"
            )));
        }

        // Only images may be proxied; anything else (HTML, JS, ...) could become stored XSS.
        if let Some(ct) = content_type.as_deref() {
            if !is_image_content_type(ct) {
                return Err(HomarrError::Upstream(format!(
                    "Image upstream returned non-image content-type '{ct}'"
                )));
            }
        }

        Ok(ImageProxyResult::Bytes {
            content_type: content_type.unwrap_or_else(|| "application/octet-stream".to_string()),
            data: bytes,
        })
    }

    fn sign_token(&self, data: &str) -> HomarrResult<String> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.key)
            .map_err(|_| HomarrError::Internal("HMAC init failed".to_string()))?;
        mac.update(data.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    fn encrypt_payload(&self, plaintext: &str) -> HomarrResult<String> {
        let mut iv = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut iv);
        let cipher = Aes256CbcEnc::new_from_slices(&self.key, &iv)
            .map_err(|err| HomarrError::Internal(format!("Image cipher init failed: {err}")))?;
        let ciphertext = cipher.encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());
        Ok(format!("{}.{}", hex::encode(ciphertext), hex::encode(iv)))
    }

    fn decrypt_payload(&self, encrypted: &str) -> HomarrResult<String> {
        let (ct_hex, iv_hex) = encrypted.split_once('.').ok_or_else(|| {
            HomarrError::BadRequest("Malformed encrypted image token".to_string())
        })?;
        let iv = hex::decode(iv_hex)
            .map_err(|err| HomarrError::BadRequest(format!("Invalid IV hex: {err}")))?;
        let ct = hex::decode(ct_hex)
            .map_err(|err| HomarrError::BadRequest(format!("Invalid ciphertext hex: {err}")))?;
        let cipher = Aes256CbcDec::new_from_slices(&self.key, &iv)
            .map_err(|err| HomarrError::BadRequest(format!("Cipher init failed: {err}")))?;
        let plain = cipher
            .decrypt_padded_vec_mut::<Pkcs7>(&ct)
            .map_err(|_| HomarrError::Forbidden("Image token decryption failed".to_string()))?;
        String::from_utf8(plain)
            .map_err(|err| HomarrError::Internal(format!("Invalid decrypted UTF-8: {err}")))
    }
}

/// True for `image/*` media types (parameters and case ignored).
pub fn is_image_content_type(content_type: &str) -> bool {
    content_type
        .trim_start()
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
}

#[async_trait::async_trait]
impl homarr_integrations::runtime::ImageProxyProvider for ImageProxyService {
    async fn proxy_image(
        &self,
        url: &str,
        headers: HashMap<String, String>,
        trusted_origin: Option<String>,
    ) -> HomarrResult<String> {
        self.forward_trusted_image(url, headers, trusted_origin)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::is_image_content_type;

    #[test]
    fn image_content_type_filter() {
        assert!(is_image_content_type("image/png"));
        assert!(is_image_content_type("IMAGE/JPEG; q=1"));
        assert!(is_image_content_type("image/svg+xml"));
        assert!(!is_image_content_type("text/html"));
        assert!(!is_image_content_type("application/octet-stream"));
        assert!(!is_image_content_type("image"));
        assert!(!is_image_content_type(""));
    }
}
