use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// TS: `number | string | boolean | null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatsValue {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatsUnit {
    Count,
    Bytes,
    Grams,
    Percent,
    Seconds,
    Milliseconds,
    BytesPerSecond,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsMetric {
    pub key: String,
    pub label: String,
    pub unit: StatsUnit,
}

/// Result of a stats provider's `fetchAsync`: metric key -> value.
pub type StatsValues = BTreeMap<String, StatsValue>;
