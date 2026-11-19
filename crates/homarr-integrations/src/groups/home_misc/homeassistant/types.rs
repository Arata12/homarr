//! Raw Home Assistant shapes (`homeassistant-types.ts`).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct RawEntityState {
    pub entity_id: String,
    pub state: String,
    pub attributes: Map<String, Value>,
    /// Unix milliseconds.
    pub last_changed: f64,
    pub last_updated: f64,
    pub context_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CompressedContext {
    Id(String),
    Object {
        #[serde(default)]
        id: Option<String>,
    },
}

impl CompressedContext {
    pub fn id(&self) -> Option<String> {
        match self {
            Self::Id(id) => Some(id.clone()),
            Self::Object { id } => id.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CompressedState {
    pub s: String,
    #[serde(default)]
    pub a: Map<String, Value>,
    pub lc: f64,
    #[serde(default)]
    pub lu: Option<f64>,
    #[serde(default)]
    pub c: Option<CompressedContext>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffPlus {
    #[serde(default)]
    pub s: Option<String>,
    #[serde(default)]
    pub a: Option<Map<String, Value>>,
    #[serde(default)]
    pub lc: Option<f64>,
    #[serde(default)]
    pub lu: Option<f64>,
    #[serde(default)]
    pub c: Option<CompressedContext>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffMinus {
    #[serde(default)]
    pub a: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CompressedDiff {
    #[serde(default, rename = "+")]
    pub plus: Option<DiffPlus>,
    #[serde(default, rename = "-")]
    pub minus: Option<DiffMinus>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CompressedStateEvent {
    #[serde(default)]
    pub a: Option<HashMap<String, CompressedState>>,
    #[serde(default)]
    pub r: Option<Vec<String>>,
    #[serde(default)]
    pub c: Option<HashMap<String, CompressedDiff>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AreaEntry {
    pub area_id: String,
    pub name: String,
    #[serde(default)]
    pub floor_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub temperature_entity_id: Option<String>,
    #[serde(default)]
    pub humidity_entity_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FloorEntry {
    pub floor_id: String,
    pub name: String,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    #[serde(default)]
    pub area_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntityRegistryEntry {
    pub ei: String,
    #[serde(default)]
    pub ai: Option<String>,
    #[serde(default)]
    pub di: Option<String>,
    #[serde(default)]
    pub en: Option<String>,
    #[serde(default)]
    pub ic: Option<String>,
    #[serde(default)]
    pub ec: Option<f64>,
    #[serde(default)]
    pub hb: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EntityDisplayRegistry {
    #[serde(default)]
    pub entities: Vec<EntityRegistryEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct HomeAssistantRegistry {
    pub areas: Vec<AreaEntry>,
    pub floors: Vec<FloorEntry>,
    pub devices: Vec<DeviceEntry>,
    pub entities: EntityDisplayRegistry,
}

#[derive(Debug, Deserialize)]
pub struct CalendarRef {
    pub entity_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CalendarMoment {
    Date {
        date: String,
    },
    DateTime {
        #[serde(rename = "dateTime")]
        date_time: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct HaCalendarEvent {
    pub start: CalendarMoment,
    pub end: CalendarMoment,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryItem {
    pub s: String,
    #[serde(default)]
    pub lu: Option<f64>,
    #[serde(default)]
    pub lc: Option<f64>,
}
