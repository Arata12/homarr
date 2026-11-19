use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    layer::{Context, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

const MAX_LOG_ENTRIES: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub module: Option<String>,
}

#[derive(Clone)]
pub struct LogStore {
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
    tx: broadcast::Sender<LogEntry>,
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LogStore {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_LOG_ENTRIES))),
            tx,
        }
    }

    pub fn push(&self, entry: LogEntry) {
        if let Ok(mut guard) = self.entries.write() {
            if guard.len() >= MAX_LOG_ENTRIES {
                guard.pop_front();
            }
            guard.push_back(entry.clone());
        }
        let _ = self.tx.send(entry);
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let guard = match self.entries.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        guard.iter().rev().take(limit).cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let val = format!("{value:?}");
        if field.name() == "message" {
            self.message = val.trim_matches('"').to_string();
        } else {
            self.fields.push((field.name().to_string(), val));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

struct StoreLayer {
    store: LogStore,
}

impl<S: Subscriber> Layer<S> for StoreLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let metadata = event.metadata();
        let message = if visitor.fields.is_empty() {
            visitor.message
        } else {
            let extras: Vec<String> = visitor
                .fields
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            if visitor.message.is_empty() {
                extras.join(" ")
            } else {
                format!("{} ({})", visitor.message, extras.join(", "))
            }
        };

        self.store.push(LogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            level: metadata.level().to_string().to_lowercase(),
            message,
            module: Some(metadata.target().to_string()),
        });
    }
}

/// Initializes global tracing subscriber writing both to stdout and `LogStore`.
pub fn init_logging(level: &str) -> LogStore {
    let store = LogStore::new();
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true);
    let store_layer = StoreLayer {
        store: store.clone(),
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(store_layer)
        .try_init();

    store
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_store_caps_buffer_and_returns_newest_first() {
        let store = LogStore::new();
        for i in 0..1050 {
            store.push(LogEntry {
                id: i.to_string(),
                timestamp: "2026-01-01T00:00:00Z".into(),
                level: "info".into(),
                message: format!("msg-{i}"),
                module: None,
            });
        }
        let recent = store.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "msg-1049");
        assert_eq!(recent[2].message, "msg-1047");
    }
}
