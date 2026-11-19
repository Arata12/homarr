use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Hard cap on the number of entities a selection may resolve to (TS `SMART_HOME_ENTITY_LIMIT`).
pub const SMART_HOME_ENTITY_LIMIT: usize = 400;

macro_rules! entity_attributes {
    ($($field:ident : $ty:ty),* $(,)?) => {
        /// Optional per-domain entity attributes (TS `SmartHomeEntityAttributes`).
        #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct SmartHomeEntityAttributes {
            $(
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $field: Option<$ty>,
            )*
        }
    };
}

entity_attributes! {
    supported_features: u64,
    brightness: f64,
    color_temp_kelvin: f64,
    min_color_temp_kelvin: f64,
    max_color_temp_kelvin: f64,
    rgb_color: [u8; 3],
    color_mode: String,
    color_modes: Vec<String>,
    effect: String,
    effects: Vec<String>,
    current_temperature: f64,
    target_temperature: f64,
    target_temperature_low: f64,
    target_temperature_high: f64,
    min_temperature: f64,
    max_temperature: f64,
    temperature_step: f64,
    temperature_unit: String,
    hvac_modes: Vec<String>,
    hvac_action: String,
    preset_mode: String,
    preset_modes: Vec<String>,
    fan_mode: String,
    fan_modes: Vec<String>,
    current_humidity: f64,
    target_humidity: f64,
    min_humidity: f64,
    max_humidity: f64,
    mode: String,
    modes: Vec<String>,
    position: f64,
    tilt_position: f64,
    percentage: f64,
    percentage_step: f64,
    oscillating: bool,
    direction: String,
    media_title: String,
    media_artist: String,
    media_album: String,
    media_series_title: String,
    media_content_type: String,
    media_source: String,
    sources: Vec<String>,
    app_name: String,
    volume: f64,
    is_muted: bool,
    media_duration: f64,
    media_position: f64,
    media_position_updated_at: f64,
    shuffle: bool,
    repeat: String,
    battery_level: f64,
    options: Vec<String>,
    min: f64,
    max: f64,
    step: f64,
    humidity: f64,
    pressure: f64,
    wind_speed: f64,
    wind_speed_unit: String,
    installed_version: String,
    latest_version: String,
    release_summary: String,
    release_url: String,
    in_progress: bool,
    last_triggered: f64,
    current_run_count: f64,
    zone: String,
    code_required: bool,
    code_format: String,
    status: String,
    fan_speed: String,
    fan_speeds: Vec<String>,
    next_setting: f64,
    next_rising: f64,
    remaining: String,
    duration: String,
    finishes_at: f64,
    message: String,
    start_time: f64,
    end_time: f64,
    location: String,
    picture_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeEntity {
    pub id: String,
    pub domain: String,
    pub name: String,
    pub area_id: Option<String>,
    pub state: String,
    pub icon: Option<String>,
    pub device_class: Option<String>,
    pub state_class: Option<String>,
    pub unit: Option<String>,
    /// Unix milliseconds.
    pub last_changed: f64,
    pub last_updated: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    pub attributes: SmartHomeEntityAttributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeActionReceipt {
    pub context_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeArea {
    pub id: String,
    pub name: String,
    pub floor_id: Option<String>,
    pub icon: Option<String>,
    pub temperature_entity_id: Option<String>,
    pub humidity_entity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeFloor {
    pub id: String,
    pub name: String,
    pub level: Option<i64>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeCatalogEntity {
    pub id: String,
    pub domain: String,
    pub name: String,
    pub area_id: Option<String>,
    pub icon: Option<String>,
    pub device_class: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeCatalog {
    pub entities: Vec<SmartHomeCatalogEntity>,
    pub areas: Vec<SmartHomeArea>,
    pub floors: Vec<SmartHomeFloor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeEntitySelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded_entity_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeSnapshot {
    pub entities: Vec<SmartHomeEntity>,
    pub areas: Vec<SmartHomeArea>,
    pub updated_at: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeHistoryPoint {
    pub time: f64,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeHistorySeries {
    pub entity_id: String,
    pub points: Vec<SmartHomeHistoryPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeEntityPatch {
    pub changed: Vec<SmartHomeEntity>,
    pub removed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<Vec<SmartHomeArea>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHomeForecastDay {
    pub time: f64,
    pub condition: String,
    pub temperature_high: Option<f64>,
    pub temperature_low: Option<f64>,
    pub precipitation_probability: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartHomeForecastType {
    Daily,
    Hourly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmArmMode {
    Away,
    Home,
    Night,
    Vacation,
    CustomBypass,
}

/// TS `SmartHomeAction` discriminated union (`{ type: "...", ... }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SmartHomeAction {
    Toggle,
    TurnOn,
    TurnOff,
    SetBrightness {
        value: f64,
    },
    SetColorTemp {
        value: f64,
    },
    SetRgbColor {
        value: [u8; 3],
    },
    SetEffect {
        value: String,
    },
    SetTemperature {
        value: f64,
    },
    SetTemperatureRange {
        low: f64,
        high: f64,
    },
    SetHvacMode {
        value: String,
    },
    SetPresetMode {
        value: String,
    },
    SetFanMode {
        value: String,
    },
    SetHumidity {
        value: f64,
    },
    SetMode {
        value: String,
    },
    SetPosition {
        value: f64,
    },
    SetTiltPosition {
        value: f64,
    },
    Open,
    Close,
    Stop,
    Lock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    Unlock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    OpenLatch {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    MediaPlayPause,
    MediaPlay,
    MediaPause,
    MediaNext,
    MediaPrevious,
    SetVolume {
        value: f64,
    },
    SetMuted {
        value: bool,
    },
    SelectSource {
        value: String,
    },
    Trigger,
    SelectOption {
        value: String,
    },
    SetValue {
        value: f64,
    },
    SetFanPercentage {
        value: f64,
    },
    SetOscillating {
        value: bool,
    },
    Increment,
    Decrement,
    VacuumStart,
    VacuumPause,
    VacuumReturn,
    VacuumLocate,
    AlarmArm {
        mode: AlarmArmMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    AlarmDisarm {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    Install,
    TimerStart,
    TimerPause,
    TimerCancel,
}

/// Convenience alias for providers that cache entities by id.
pub type SmartHomeEntityMap = BTreeMap<String, SmartHomeEntity>;
