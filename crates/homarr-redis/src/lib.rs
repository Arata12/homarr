use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use homarr_common::{HomarrError, HomarrResult};
use homarr_core::AppConfig;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone)]
pub struct PubSubEnvelope {
    pub channel: String,
    pub payload: String,
}

#[derive(Default)]
struct MemoryRedis {
    kv: RwLock<HashMap<String, (String, Option<Instant>)>>,
    sets: RwLock<HashMap<String, HashSet<String>>>,
}

impl MemoryRedis {
    async fn get(&self, key: &str) -> Option<String> {
        let mut guard = self.kv.write().await;
        if let Some((val, exp)) = guard.get(key) {
            if let Some(deadline) = exp {
                if Instant::now() >= *deadline {
                    guard.remove(key);
                    return None;
                }
            }
            return Some(val.clone());
        }
        None
    }

    async fn set_ex(&self, key: &str, value: &str, ttl_secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(ttl_secs);
        self.kv
            .write()
            .await
            .insert(key.to_string(), (value.to_string(), Some(deadline)));
    }

    async fn del(&self, key: &str) {
        self.kv.write().await.remove(key);
        self.sets.write().await.remove(key);
    }

    async fn sadd(&self, key: &str, member: &str) {
        self.sets
            .write()
            .await
            .entry(key.to_string())
            .or_default()
            .insert(member.to_string());
    }

    async fn smembers(&self, key: &str) -> Vec<String> {
        self.sets
            .read()
            .await
            .get(key)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn srem(&self, key: &str, member: &str) {
        if let Some(set) = self.sets.write().await.get_mut(key) {
            set.remove(member);
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum Backend {
    Live(ConnectionManager),
    Memory(Arc<MemoryRedis>),
}

#[derive(Clone)]
pub struct RedisStore {
    backend: Backend,
    tx: broadcast::Sender<PubSubEnvelope>,
}

impl std::fmt::Debug for RedisStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.backend {
            Backend::Live(_) => f.write_str("RedisStore::Live(..)"),
            Backend::Memory(_) => f.write_str("RedisStore::Memory(..)"),
        }
    }
}

impl RedisStore {
    pub fn in_memory() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            backend: Backend::Memory(Arc::new(MemoryRedis::default())),
            tx,
        }
    }

    pub async fn connect(config: &AppConfig) -> HomarrResult<Self> {
        if config.redis_disabled {
            tracing::warn!("REDIS_DISABLED=true; using in-memory store");
            return Ok(Self::in_memory());
        }

        let url = config.redis_url();
        let client = redis::Client::open(url.as_str())
            .map_err(|err| HomarrError::Upstream(format!("Invalid Redis URL: {err}")))?;
        let manager = ConnectionManager::new(client)
            .await
            .map_err(|err| HomarrError::Upstream(format!("Failed to connect to Redis: {err}")))?;
        let (tx, _) = broadcast::channel(512);
        Ok(Self {
            backend: Backend::Live(manager),
            tx,
        })
    }

    pub async fn health_check(&self) -> HomarrResult<f64> {
        let start = Instant::now();
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                let pong: String = redis::cmd("PING")
                    .query_async(&mut c)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis PING failed: {err}")))?;
                if pong != "PONG" {
                    return Err(HomarrError::Upstream(format!(
                        "Unexpected Redis PING response: {pong}"
                    )));
                }
            }
            Backend::Memory(_) => {}
        }
        Ok(start.elapsed().as_secs_f64() * 1000.0)
    }

    pub async fn get_string(&self, key: &str) -> HomarrResult<Option<String>> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.get(key)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis GET failed: {err}")))
            }
            Backend::Memory(mem) => Ok(mem.get(key).await),
        }
    }

    pub async fn set_string_ex(&self, key: &str, value: &str, ttl_secs: u64) -> HomarrResult<()> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.set_ex::<_, _, ()>(key, value, ttl_secs)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis SETEX failed: {err}")))?;
            }
            Backend::Memory(mem) => mem.set_ex(key, value, ttl_secs).await,
        }
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> HomarrResult<()> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.del::<_, ()>(key)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis DEL failed: {err}")))?;
            }
            Backend::Memory(mem) => mem.del(key).await,
        }
        Ok(())
    }

    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> HomarrResult<Option<T>> {
        let Some(raw) = self.get_string(key).await? else {
            return Ok(None);
        };
        let parsed = serde_json::from_str(&raw)
            .map_err(|err| HomarrError::Internal(format!("Redis JSON decode failed: {err}")))?;
        Ok(Some(parsed))
    }

    pub async fn set_json_ex<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> HomarrResult<()> {
        let raw = serde_json::to_string(value)
            .map_err(|err| HomarrError::Internal(format!("Redis JSON encode failed: {err}")))?;
        self.set_string_ex(key, &raw, ttl_secs).await
    }

    pub async fn sadd(&self, key: &str, member: &str) -> HomarrResult<()> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.sadd::<_, _, ()>(key, member)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis SADD failed: {err}")))?;
            }
            Backend::Memory(mem) => mem.sadd(key, member).await,
        }
        Ok(())
    }

    pub async fn smembers(&self, key: &str) -> HomarrResult<Vec<String>> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.smembers(key)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis SMEMBERS failed: {err}")))
            }
            Backend::Memory(mem) => Ok(mem.smembers(key).await),
        }
    }

    pub async fn srem(&self, key: &str, member: &str) -> HomarrResult<()> {
        match &self.backend {
            Backend::Live(conn) => {
                let mut c = conn.clone();
                c.srem::<_, _, ()>(key, member)
                    .await
                    .map_err(|err| HomarrError::Upstream(format!("Redis SREM failed: {err}")))?;
            }
            Backend::Memory(mem) => mem.srem(key, member).await,
        }
        Ok(())
    }

    pub async fn publish(&self, channel: &str, payload: &str) -> HomarrResult<()> {
        if let Backend::Live(conn) = &self.backend {
            let mut c = conn.clone();
            let _ = c.publish::<_, _, i64>(channel, payload).await;
        }
        let _ = self.tx.send(PubSubEnvelope {
            channel: channel.to_string(),
            payload: payload.to_string(),
        });
        Ok(())
    }

    pub async fn publish_json<T: Serialize>(&self, channel: &str, value: &T) -> HomarrResult<()> {
        let raw = serde_json::to_string(value)
            .map_err(|err| HomarrError::Internal(format!("PubSub JSON encode failed: {err}")))?;
        self.publish(channel, &raw).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PubSubEnvelope> {
        self.tx.subscribe()
    }

    // Domain channels
    pub const PING_URLS_KEY: &'static str = "homarr:ping:urls";
    pub const PING_EVENTS_CHANNEL: &'static str = "homarr:ping:events";
    pub const CRON_STATUS_CHANNEL: &'static str = "homarr:cron:status";
    pub const BOARD_EVENTS_CHANNEL: &'static str = "homarr:board:events";

    pub async fn register_ping_url(&self, url: &str) -> HomarrResult<()> {
        self.sadd(Self::PING_URLS_KEY, url).await
    }

    pub async fn list_ping_urls(&self) -> HomarrResult<Vec<String>> {
        self.smembers(Self::PING_URLS_KEY).await
    }

    pub async fn clear_ping_urls(&self) -> HomarrResult<()> {
        self.delete(Self::PING_URLS_KEY).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_redis_kv_sets_and_pubsub() {
        let store = RedisStore::in_memory();
        assert!(store.health_check().await.is_ok());

        store.set_string_ex("k1", "v1", 60).await.unwrap();
        assert_eq!(store.get_string("k1").await.unwrap().as_deref(), Some("v1"));
        store.delete("k1").await.unwrap();
        assert_eq!(store.get_string("k1").await.unwrap(), None);

        store.register_ping_url("https://homarr.dev").await.unwrap();
        assert_eq!(
            store.list_ping_urls().await.unwrap(),
            vec!["https://homarr.dev".to_string()]
        );

        let mut rx = store.subscribe();
        store.publish("ch", "hello").await.unwrap();
        let env = rx.recv().await.unwrap();
        assert_eq!(env.channel, "ch");
        assert_eq!(env.payload, "hello");
    }

    #[tokio::test]
    async fn live_redis_when_available() {
        let Ok(host) = std::env::var("REDIS_HOST") else {
            return;
        };
        if host.trim().is_empty() {
            return;
        }
        let mut cfg = AppConfig::for_test();
        cfg.redis_disabled = false;
        cfg.redis_host = host;
        let store = RedisStore::connect(&cfg).await.expect("connects to redis");
        let latency = store.health_check().await.expect("PING succeeds");
        assert!(latency >= 0.0);

        store
            .set_string_ex("homarr:test:key", "ok", 10)
            .await
            .unwrap();
        assert_eq!(
            store
                .get_string("homarr:test:key")
                .await
                .unwrap()
                .as_deref(),
            Some("ok")
        );
        store.delete("homarr:test:key").await.unwrap();
    }
}
