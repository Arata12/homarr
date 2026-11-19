use chrono::Utc;
use homarr_common::{is_http_url, validate_outbound_url, HomarrResult, Stopwatch};
use homarr_core::HttpClientFactory;
use homarr_redis::RedisStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub url: String,
    pub status_code: Option<u16>,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub checked_at: i64,
}

#[derive(Clone)]
pub struct PingService {
    http: HttpClientFactory,
    redis: RedisStore,
}

impl PingService {
    pub fn new(http: HttpClientFactory, redis: RedisStore) -> Self {
        Self { http, redis }
    }

    pub fn cache_key(url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        format!("ping:last:{}", hex::encode(hasher.finalize()))
    }

    pub async fn ping_url(&self, url: &str) -> PingResult {
        let checked_at = Utc::now().timestamp();
        let trimmed = url.trim().to_string();
        if !is_http_url(&trimmed) {
            return PingResult {
                url: trimmed,
                status_code: None,
                duration_ms: 0,
                error: Some("URL must use http:// or https://".to_string()),
                checked_at,
            };
        }

        // Reject cloud metadata & unsafe IP targets even in ping checks.
        if let Err(err) = validate_outbound_url(&trimmed, None) {
            // Allow explicit loopback only if URL explicitly uses localhost/127.0.0.1 for local container self-checks?
            // Keep validate_outbound_url strict on metadata & unsafe targets.
            return PingResult {
                url: trimmed,
                status_code: None,
                duration_ms: 0,
                error: Some(err.to_string()),
                checked_at,
            };
        }

        let sw = Stopwatch::new();
        let result = match self.http.ping().get(&trimmed).send().await {
            Ok(resp) => PingResult {
                url: trimmed.clone(),
                status_code: Some(resp.status().as_u16()),
                duration_ms: sw.elapsed_ms().round().max(0.0) as u64,
                error: None,
                checked_at,
            },
            Err(err) => PingResult {
                url: trimmed.clone(),
                status_code: None,
                duration_ms: sw.elapsed_ms().round().max(0.0) as u64,
                error: Some(err.to_string()),
                checked_at,
            },
        };

        let _ = self
            .redis
            .set_json_ex(&Self::cache_key(&trimmed), &result, 300)
            .await;
        result
    }

    pub async fn get_cached_ping(&self, url: &str) -> HomarrResult<Option<PingResult>> {
        self.redis.get_json(&Self::cache_key(url)).await
    }
}
