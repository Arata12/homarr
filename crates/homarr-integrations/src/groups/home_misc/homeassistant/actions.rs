//! Action translation and capability checks (`homeassistant-actions.ts`, `smart-home-capabilities.ts`).

use serde_json::{json, Map, Value};

use super::normalize::{get_entity_domain, light_capabilities};
use crate::capabilities::smart_home::{AlarmArmMode, SmartHomeAction, SmartHomeEntity};

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceCall {
    pub domain: String,
    pub service: String,
    pub data: Map<String, Value>,
}

const ON_OFF_DOMAINS: [&str; 13] = [
    "light",
    "switch",
    "fan",
    "input_boolean",
    "humidifier",
    "automation",
    "siren",
    "remote",
    "media_player",
    "climate",
    "water_heater",
    "script",
    "group",
];

const TOGGLE_DOMAINS: [&str; 14] = [
    "light",
    "switch",
    "fan",
    "input_boolean",
    "humidifier",
    "automation",
    "siren",
    "remote",
    "media_player",
    "climate",
    "cover",
    "valve",
    "script",
    "group",
];

fn call(domain: &str, service: &str, data: Value) -> Option<ServiceCall> {
    Some(ServiceCall {
        domain: domain.to_string(),
        service: service.to_string(),
        data: data.as_object().cloned().unwrap_or_default(),
    })
}

fn plain(domain: &str, service: &str) -> Option<ServiceCall> {
    call(domain, service, json!({}))
}

fn with_code(code: &Option<String>) -> Value {
    match code {
        Some(code) if !code.is_empty() => json!({ "code": code }),
        _ => json!({}),
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

impl AlarmArmMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Away => "away",
            Self::Home => "home",
            Self::Night => "night",
            Self::Vacation => "vacation",
            Self::CustomBypass => "custom_bypass",
        }
    }
}

fn resolve_call(domain: &str, action: &SmartHomeAction) -> Option<ServiceCall> {
    use SmartHomeAction as A;
    match action {
        A::Toggle => {
            if domain == "group" {
                return plain("homeassistant", "toggle");
            }
            TOGGLE_DOMAINS
                .contains(&domain)
                .then(|| plain(domain, "toggle"))
                .flatten()
        }
        A::TurnOn => {
            if domain == "group" {
                return plain("homeassistant", "turn_on");
            }
            if domain == "scene" {
                return plain("scene", "turn_on");
            }
            ON_OFF_DOMAINS
                .contains(&domain)
                .then(|| plain(domain, "turn_on"))
                .flatten()
        }
        A::TurnOff => {
            if domain == "group" {
                return plain("homeassistant", "turn_off");
            }
            ON_OFF_DOMAINS
                .contains(&domain)
                .then(|| plain(domain, "turn_off"))
                .flatten()
        }
        A::SetBrightness { value } => {
            if domain != "light" {
                return None;
            }
            if *value <= 0.0 {
                return plain("light", "turn_off");
            }
            call(
                "light",
                "turn_on",
                json!({ "brightness_pct": clamp(*value, 1.0, 100.0) }),
            )
        }
        A::SetColorTemp { value } => (domain == "light")
            .then(|| {
                call(
                    "light",
                    "turn_on",
                    json!({ "color_temp_kelvin": value.round() }),
                )
            })
            .flatten(),
        A::SetRgbColor { value } => (domain == "light")
            .then(|| call("light", "turn_on", json!({ "rgb_color": value })))
            .flatten(),
        A::SetEffect { value } => (domain == "light")
            .then(|| call("light", "turn_on", json!({ "effect": value })))
            .flatten(),
        A::SetTemperature { value } => match domain {
            "climate" | "water_heater" => {
                call(domain, "set_temperature", json!({ "temperature": value }))
            }
            _ => None,
        },
        A::SetTemperatureRange { low, high } => (domain == "climate")
            .then(|| {
                call(
                    "climate",
                    "set_temperature",
                    json!({ "target_temp_low": low, "target_temp_high": high }),
                )
            })
            .flatten(),
        A::SetHvacMode { value } => (domain == "climate")
            .then(|| call("climate", "set_hvac_mode", json!({ "hvac_mode": value })))
            .flatten(),
        A::SetPresetMode { value } => match domain {
            "climate" | "fan" => call(domain, "set_preset_mode", json!({ "preset_mode": value })),
            _ => None,
        },
        A::SetFanMode { value } => (domain == "climate")
            .then(|| call("climate", "set_fan_mode", json!({ "fan_mode": value })))
            .flatten(),
        A::SetHumidity { value } => match domain {
            "humidifier" | "climate" => call(domain, "set_humidity", json!({ "humidity": value })),
            _ => None,
        },
        A::SetMode { value } => match domain {
            "humidifier" => call("humidifier", "set_mode", json!({ "mode": value })),
            "water_heater" => call(
                "water_heater",
                "set_operation_mode",
                json!({ "operation_mode": value }),
            ),
            _ => None,
        },
        A::SetPosition { value } => match domain {
            "cover" => call(
                "cover",
                "set_cover_position",
                json!({ "position": clamp(*value, 0.0, 100.0) }),
            ),
            "valve" => call(
                "valve",
                "set_valve_position",
                json!({ "position": clamp(*value, 0.0, 100.0) }),
            ),
            _ => None,
        },
        A::SetTiltPosition { value } => (domain == "cover")
            .then(|| {
                call(
                    "cover",
                    "set_cover_tilt_position",
                    json!({ "tilt_position": clamp(*value, 0.0, 100.0) }),
                )
            })
            .flatten(),
        A::Open => match domain {
            "cover" => plain("cover", "open_cover"),
            "valve" => plain("valve", "open_valve"),
            _ => None,
        },
        A::Close => match domain {
            "cover" => plain("cover", "close_cover"),
            "valve" => plain("valve", "close_valve"),
            _ => None,
        },
        A::Stop => match domain {
            "cover" => plain("cover", "stop_cover"),
            "valve" => plain("valve", "stop_valve"),
            _ => None,
        },
        A::Lock { code } => (domain == "lock")
            .then(|| call("lock", "lock", with_code(code)))
            .flatten(),
        A::Unlock { code } => (domain == "lock")
            .then(|| call("lock", "unlock", with_code(code)))
            .flatten(),
        A::OpenLatch { code } => (domain == "lock")
            .then(|| call("lock", "open", with_code(code)))
            .flatten(),
        A::MediaPlayPause => (domain == "media_player")
            .then(|| plain("media_player", "media_play_pause"))
            .flatten(),
        A::MediaPlay => (domain == "media_player")
            .then(|| plain("media_player", "media_play"))
            .flatten(),
        A::MediaPause => (domain == "media_player")
            .then(|| plain("media_player", "media_pause"))
            .flatten(),
        A::MediaNext => (domain == "media_player")
            .then(|| plain("media_player", "media_next_track"))
            .flatten(),
        A::MediaPrevious => (domain == "media_player")
            .then(|| plain("media_player", "media_previous_track"))
            .flatten(),
        A::SetVolume { value } => (domain == "media_player")
            .then(|| {
                call(
                    "media_player",
                    "volume_set",
                    json!({ "volume_level": clamp(*value, 0.0, 1.0) }),
                )
            })
            .flatten(),
        A::SetMuted { value } => (domain == "media_player")
            .then(|| {
                call(
                    "media_player",
                    "volume_mute",
                    json!({ "is_volume_muted": value }),
                )
            })
            .flatten(),
        A::SelectSource { value } => (domain == "media_player")
            .then(|| call("media_player", "select_source", json!({ "source": value })))
            .flatten(),
        A::Trigger => match domain {
            "scene" => plain("scene", "turn_on"),
            "script" => plain("script", "turn_on"),
            "automation" => plain("automation", "trigger"),
            "button" | "input_button" => plain(domain, "press"),
            _ => None,
        },
        A::SelectOption { value } => match domain {
            "input_select" | "select" => call(domain, "select_option", json!({ "option": value })),
            _ => None,
        },
        A::SetValue { value } => match domain {
            "input_number" | "number" => call(domain, "set_value", json!({ "value": value })),
            _ => None,
        },
        A::SetFanPercentage { value } => (domain == "fan")
            .then(|| {
                call(
                    "fan",
                    "set_percentage",
                    json!({ "percentage": clamp(*value, 0.0, 100.0) }),
                )
            })
            .flatten(),
        A::SetOscillating { value } => (domain == "fan")
            .then(|| call("fan", "oscillate", json!({ "oscillating": value })))
            .flatten(),
        A::Increment => matches!(domain, "counter" | "input_number")
            .then(|| plain(domain, "increment"))
            .flatten(),
        A::Decrement => matches!(domain, "counter" | "input_number")
            .then(|| plain(domain, "decrement"))
            .flatten(),
        A::VacuumStart => match domain {
            "vacuum" => plain("vacuum", "start"),
            "lawn_mower" => plain("lawn_mower", "start_mowing"),
            _ => None,
        },
        A::VacuumPause => matches!(domain, "vacuum" | "lawn_mower")
            .then(|| plain(domain, "pause"))
            .flatten(),
        A::VacuumReturn => match domain {
            "vacuum" => plain("vacuum", "return_to_base"),
            "lawn_mower" => plain("lawn_mower", "dock"),
            _ => None,
        },
        A::VacuumLocate => (domain == "vacuum")
            .then(|| plain("vacuum", "locate"))
            .flatten(),
        A::AlarmArm { mode, code } => (domain == "alarm_control_panel")
            .then(|| {
                call(
                    "alarm_control_panel",
                    &format!("alarm_arm_{}", mode.as_str()),
                    with_code(code),
                )
            })
            .flatten(),
        A::AlarmDisarm { code } => (domain == "alarm_control_panel")
            .then(|| call("alarm_control_panel", "alarm_disarm", with_code(code)))
            .flatten(),
        A::Install => (domain == "update")
            .then(|| plain("update", "install"))
            .flatten(),
        A::TimerStart => (domain == "timer")
            .then(|| plain("timer", "start"))
            .flatten(),
        A::TimerPause => (domain == "timer")
            .then(|| plain("timer", "pause"))
            .flatten(),
        A::TimerCancel => (domain == "timer")
            .then(|| plain("timer", "cancel"))
            .flatten(),
    }
}

pub fn to_service_call(entity_id: &str, action: &SmartHomeAction) -> Option<ServiceCall> {
    resolve_call(get_entity_domain(entity_id), action)
}

pub fn action_type(action: &SmartHomeAction) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}

const MOMENTARY_DOMAINS: [&str; 3] = ["scene", "button", "input_button"];

/// `resolveAction`: turns a generic toggle into the domain specific action.
pub fn resolve_action(entity: &SmartHomeEntity, action: &SmartHomeAction) -> SmartHomeAction {
    if !matches!(action, SmartHomeAction::Toggle) {
        return action.clone();
    }
    let (domain, state) = (entity.domain.as_str(), entity.state.as_str());
    if domain == "climate" {
        if state != "off" {
            return SmartHomeAction::SetHvacMode {
                value: "off".into(),
            };
        }
        let modes = entity.attributes.hvac_modes.clone().unwrap_or_default();
        let value = ["heat_cool", "heat", "cool"]
            .into_iter()
            .find(|mode| modes.iter().any(|candidate| candidate == mode))
            .map(str::to_string)
            .or_else(|| modes.iter().find(|mode| mode.as_str() != "off").cloned());
        if let Some(value) = value {
            return SmartHomeAction::SetHvacMode { value };
        }
    }
    if domain == "lock" {
        return if state == "locked" {
            SmartHomeAction::Unlock { code: None }
        } else {
            SmartHomeAction::Lock { code: None }
        };
    }
    if MOMENTARY_DOMAINS.contains(&domain) {
        return SmartHomeAction::Trigger;
    }
    action.clone()
}

// ---- capability checks -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureMode {
    Single,
    Range,
}

pub fn climate_temperature_mode(entity: &SmartHomeEntity) -> Option<TemperatureMode> {
    if entity.domain != "climate" {
        return None;
    }
    if ["dry", "fan_only", "unknown", "unavailable"].contains(&entity.state.as_str()) {
        return None;
    }
    let features = entity.attributes.supported_features.unwrap_or(0);
    let single = features & 1 != 0;
    let range = features & 2 != 0;
    let reported_range = entity.attributes.target_temperature_low.is_some()
        && entity.attributes.target_temperature_high.is_some();
    if range
        && (entity.state == "heat_cool" || !single || (entity.state == "auto" && reported_range))
    {
        return Some(TemperatureMode::Range);
    }
    single.then_some(TemperatureMode::Single)
}

fn has_feature(entity: &SmartHomeEntity, feature: u64) -> bool {
    entity.attributes.supported_features.unwrap_or(0) & feature != 0
}

fn within(value: f64, min: Option<f64>, max: Option<f64>) -> bool {
    value.is_finite() && min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}

const COVER_OPEN: u64 = 1;
const COVER_CLOSE: u64 = 2;
const COVER_SET_POSITION: u64 = 4;
const COVER_STOP: u64 = 8;
const COVER_SET_TILT_POSITION: u64 = 128;
const MEDIA_PAUSE: u64 = 1;
const MEDIA_VOLUME: u64 = 4;
const MEDIA_MUTE: u64 = 8;
const MEDIA_PREVIOUS: u64 = 16;
const MEDIA_NEXT: u64 = 32;
const MEDIA_SOURCE: u64 = 2048;
const MEDIA_PLAY: u64 = 16384;

fn alarm_feature(mode: AlarmArmMode) -> u64 {
    match mode {
        AlarmArmMode::Home => 1,
        AlarmArmMode::Away => 2,
        AlarmArmMode::Night => 4,
        AlarmArmMode::CustomBypass => 16,
        AlarmArmMode::Vacation => 32,
    }
}

fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

pub fn supports_action(entity: &SmartHomeEntity, action: &SmartHomeAction) -> bool {
    use SmartHomeAction as A;
    let attributes = &entity.attributes;
    let domain = entity.domain.as_str();
    match action {
        A::Toggle => {
            if domain == "cover" {
                let required = COVER_OPEN | COVER_CLOSE;
                return attributes.supported_features.unwrap_or(0) & required == required;
            }
            true
        }
        A::SetFanPercentage { value } => {
            domain == "fan" && has_feature(entity, 1) && within(*value, Some(0.0), Some(100.0))
        }
        A::SetOscillating { .. } => domain == "fan" && has_feature(entity, 2),
        A::SetPosition { value } => {
            if domain == "valve" {
                return true;
            }
            domain == "cover"
                && has_feature(entity, COVER_SET_POSITION)
                && within(*value, Some(0.0), Some(100.0))
        }
        A::SetTiltPosition { value } => {
            // Valves have no tilt position; the TS check falls through to the cover rules.
            domain == "cover"
                && has_feature(entity, COVER_SET_TILT_POSITION)
                && within(*value, Some(0.0), Some(100.0))
        }
        A::Open | A::Close | A::Stop => {
            if domain == "valve" {
                return true;
            }
            let feature = match action {
                A::Open => COVER_OPEN,
                A::Close => COVER_CLOSE,
                _ => COVER_STOP,
            };
            domain == "cover" && has_feature(entity, feature)
        }
        A::MediaPlayPause => {
            domain == "media_player"
                && has_feature(
                    entity,
                    if entity.state == "playing" {
                        MEDIA_PAUSE
                    } else {
                        MEDIA_PLAY
                    },
                )
        }
        A::MediaPlay => domain == "media_player" && has_feature(entity, MEDIA_PLAY),
        A::MediaPause => domain == "media_player" && has_feature(entity, MEDIA_PAUSE),
        A::MediaNext => domain == "media_player" && has_feature(entity, MEDIA_NEXT),
        A::MediaPrevious => domain == "media_player" && has_feature(entity, MEDIA_PREVIOUS),
        A::SetVolume { value } => {
            domain == "media_player"
                && has_feature(entity, MEDIA_VOLUME)
                && within(*value, Some(0.0), Some(1.0))
        }
        A::SetMuted { .. } => domain == "media_player" && has_feature(entity, MEDIA_MUTE),
        A::SelectSource { value } => {
            domain == "media_player"
                && has_feature(entity, MEDIA_SOURCE)
                && attributes
                    .sources
                    .as_ref()
                    .is_some_and(|sources| sources.contains(value))
        }
        A::AlarmArm { mode, .. } => {
            domain == "alarm_control_panel" && has_feature(entity, alarm_feature(*mode))
        }
        A::AlarmDisarm { .. } => domain == "alarm_control_panel",
        A::SetTemperature { value } => {
            (domain == "water_heater"
                || climate_temperature_mode(entity) == Some(TemperatureMode::Single))
                && within(
                    *value,
                    attributes.min_temperature,
                    attributes.max_temperature,
                )
        }
        A::SetTemperatureRange { low, high } => {
            climate_temperature_mode(entity) == Some(TemperatureMode::Range)
                && low <= high
                && within(*low, attributes.min_temperature, attributes.max_temperature)
                && within(
                    *high,
                    attributes.min_temperature,
                    attributes.max_temperature,
                )
        }
        A::SetHvacMode { value } => {
            domain == "climate"
                && attributes
                    .hvac_modes
                    .as_ref()
                    .is_some_and(|modes| modes.contains(value))
        }
        A::SetHumidity { value } => {
            (domain == "humidifier"
                || (domain == "climate" && attributes.supported_features.unwrap_or(0) & 4 != 0))
                && is_integer(*value)
                && within(
                    *value,
                    Some(attributes.min_humidity.unwrap_or(0.0)),
                    Some(attributes.max_humidity.unwrap_or(100.0)),
                )
        }
        A::SetBrightness { .. } => {
            domain == "light"
                && light_capabilities(attributes.color_modes.as_deref().unwrap_or_default())
                    .brightness
        }
        A::SetRgbColor { value } => {
            let _ = value; // u8 channels are always integers within 0..=255.
            domain == "light"
                && light_capabilities(attributes.color_modes.as_deref().unwrap_or_default()).color
        }
        A::SetColorTemp { value } => {
            domain == "light"
                && light_capabilities(attributes.color_modes.as_deref().unwrap_or_default())
                    .color_temperature
                && within(
                    *value,
                    attributes.min_color_temp_kelvin,
                    attributes.max_color_temp_kelvin,
                )
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::smart_home::SmartHomeEntityAttributes;

    fn entity(domain: &str, state: &str, attributes: SmartHomeEntityAttributes) -> SmartHomeEntity {
        SmartHomeEntity {
            id: format!("{domain}.x"),
            domain: domain.into(),
            name: "x".into(),
            area_id: None,
            state: state.into(),
            icon: None,
            device_class: None,
            state_class: None,
            unit: None,
            last_changed: 0.0,
            last_updated: 0.0,
            context_id: None,
            attributes,
        }
    }

    #[test]
    fn translates_actions_to_service_calls() {
        let call = to_service_call(
            "light.kitchen",
            &SmartHomeAction::SetBrightness { value: 150.0 },
        )
        .unwrap();
        assert_eq!(
            (call.domain.as_str(), call.service.as_str()),
            ("light", "turn_on")
        );
        assert_eq!(call.data["brightness_pct"], json!(100.0));
        let off = to_service_call(
            "light.kitchen",
            &SmartHomeAction::SetBrightness { value: 0.0 },
        )
        .unwrap();
        assert_eq!(off.service, "turn_off");

        let group = to_service_call("group.all", &SmartHomeAction::Toggle).unwrap();
        assert_eq!(
            (group.domain.as_str(), group.service.as_str()),
            ("homeassistant", "toggle")
        );
        assert!(to_service_call("sensor.temp", &SmartHomeAction::TurnOn).is_none());

        let arm = to_service_call(
            "alarm_control_panel.home",
            &SmartHomeAction::AlarmArm {
                mode: AlarmArmMode::CustomBypass,
                code: Some("1234".into()),
            },
        )
        .unwrap();
        assert_eq!(arm.service, "alarm_arm_custom_bypass");
        assert_eq!(arm.data["code"], json!("1234"));
        assert_eq!(
            action_type(&SmartHomeAction::SetRgbColor { value: [1, 2, 3] }),
            "setRgbColor"
        );
    }

    #[test]
    fn toggle_resolves_per_domain() {
        let lock = entity("lock", "locked", SmartHomeEntityAttributes::default());
        assert_eq!(
            resolve_action(&lock, &SmartHomeAction::Toggle),
            SmartHomeAction::Unlock { code: None }
        );
        let scene = entity("scene", "unknown", SmartHomeEntityAttributes::default());
        assert_eq!(
            resolve_action(&scene, &SmartHomeAction::Toggle),
            SmartHomeAction::Trigger
        );

        let climate_on = entity("climate", "heat", SmartHomeEntityAttributes::default());
        assert_eq!(
            resolve_action(&climate_on, &SmartHomeAction::Toggle),
            SmartHomeAction::SetHvacMode {
                value: "off".into()
            }
        );
        let climate_off = entity(
            "climate",
            "off",
            SmartHomeEntityAttributes {
                hvac_modes: Some(vec!["off".into(), "cool".into(), "heat".into()]),
                ..Default::default()
            },
        );
        assert_eq!(
            resolve_action(&climate_off, &SmartHomeAction::Toggle),
            SmartHomeAction::SetHvacMode {
                value: "heat".into()
            }
        );
    }

    #[test]
    fn capability_checks_respect_features_and_limits() {
        let cover = entity(
            "cover",
            "open",
            SmartHomeEntityAttributes {
                supported_features: Some(3),
                ..Default::default()
            },
        );
        assert!(supports_action(&cover, &SmartHomeAction::Toggle));
        assert!(!supports_action(
            &cover,
            &SmartHomeAction::SetPosition { value: 50.0 }
        ));
        let only_open = entity(
            "cover",
            "open",
            SmartHomeEntityAttributes {
                supported_features: Some(1),
                ..Default::default()
            },
        );
        assert!(!supports_action(&only_open, &SmartHomeAction::Toggle));

        let climate = entity(
            "climate",
            "heat",
            SmartHomeEntityAttributes {
                supported_features: Some(1),
                min_temperature: Some(10.0),
                max_temperature: Some(30.0),
                ..Default::default()
            },
        );
        assert!(supports_action(
            &climate,
            &SmartHomeAction::SetTemperature { value: 21.0 }
        ));
        assert!(!supports_action(
            &climate,
            &SmartHomeAction::SetTemperature { value: 35.0 }
        ));
        assert!(!supports_action(
            &climate,
            &SmartHomeAction::SetTemperatureRange {
                low: 18.0,
                high: 22.0
            }
        ));

        let player = entity(
            "media_player",
            "playing",
            SmartHomeEntityAttributes {
                supported_features: Some(MEDIA_PAUSE | MEDIA_SOURCE),
                sources: Some(vec!["TV".into()]),
                ..Default::default()
            },
        );
        assert!(supports_action(&player, &SmartHomeAction::MediaPlayPause));
        assert!(supports_action(
            &player,
            &SmartHomeAction::SelectSource { value: "TV".into() }
        ));
        assert!(!supports_action(
            &player,
            &SmartHomeAction::SelectSource {
                value: "HDMI".into()
            }
        ));
        assert!(!supports_action(
            &player,
            &SmartHomeAction::SetVolume { value: 0.5 }
        ));

        let humidifier = entity("humidifier", "on", SmartHomeEntityAttributes::default());
        assert!(supports_action(
            &humidifier,
            &SmartHomeAction::SetHumidity { value: 50.0 }
        ));
        assert!(!supports_action(
            &humidifier,
            &SmartHomeAction::SetHumidity { value: 50.5 }
        ));
    }
}
