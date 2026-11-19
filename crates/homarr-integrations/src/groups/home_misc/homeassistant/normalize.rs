//! Entity normalization and registry lookups (`homeassistant-normalize.ts`).

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::types::{HomeAssistantRegistry, RawEntityState};
use crate::capabilities::smart_home::{
    SmartHomeArea, SmartHomeCatalog, SmartHomeCatalogEntity, SmartHomeEntity,
    SmartHomeEntityAttributes, SmartHomeFloor,
};

type Attributes = Map<String, Value>;

fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn boolean(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

fn texts(value: Option<&Value>) -> Option<Vec<String>> {
    let items = value?.as_array()?;
    items
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
}

/// `Date.parse` for ISO strings, in Unix milliseconds.
fn timestamp(value: Option<&Value>) -> Option<f64> {
    let raw = value?.as_str()?;
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.timestamp_millis() as f64);
    }
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|naive| naive.and_utc().timestamp_millis() as f64)
}

fn rgb(value: Option<&Value>) -> Option<[u8; 3]> {
    let items = value?.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let channel =
        |index: usize| number(items.get(index)).map(|value| value.clamp(0.0, 255.0).round() as u8);
    Some([channel(0)?, channel(1)?, channel(2)?])
}

fn percent_of_255(value: Option<&Value>) -> Option<f64> {
    number(value).map(|value| (value / 255.0 * 100.0).round())
}

const COLOR_MODES: [&str; 5] = ["hs", "xy", "rgb", "rgbw", "rgbww"];
const BRIGHTNESS_MODES: [&str; 8] = [
    "brightness",
    "color_temp",
    "hs",
    "xy",
    "rgb",
    "rgbw",
    "rgbww",
    "white",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightCapabilities {
    pub brightness: bool,
    pub color: bool,
    pub color_temperature: bool,
}

pub fn light_capabilities(color_modes: &[String]) -> LightCapabilities {
    LightCapabilities {
        brightness: color_modes
            .iter()
            .any(|mode| BRIGHTNESS_MODES.contains(&mode.as_str())),
        color: color_modes
            .iter()
            .any(|mode| COLOR_MODES.contains(&mode.as_str())),
        color_temperature: color_modes.iter().any(|mode| mode == "color_temp"),
    }
}

fn domain_attributes(domain: &str, attributes: &Attributes) -> SmartHomeEntityAttributes {
    let get = |key: &str| attributes.get(key);
    let mut out = SmartHomeEntityAttributes::default();
    match domain {
        "light" => {
            let color_modes = texts(get("supported_color_modes"));
            let capabilities = light_capabilities(color_modes.as_deref().unwrap_or_default());
            out.brightness = if capabilities.brightness {
                percent_of_255(get("brightness"))
            } else {
                None
            };
            out.color_temp_kelvin = if capabilities.color_temperature {
                number(get("color_temp_kelvin"))
            } else {
                None
            };
            out.min_color_temp_kelvin = number(get("min_color_temp_kelvin"));
            out.max_color_temp_kelvin = number(get("max_color_temp_kelvin"));
            out.rgb_color = if capabilities.color {
                rgb(get("rgb_color"))
            } else {
                None
            };
            out.color_mode = text(get("color_mode"));
            out.color_modes = color_modes;
            out.effect = text(get("effect"));
            out.effects = texts(get("effect_list"));
        }
        "climate" => {
            let features = number(get("supported_features")).unwrap_or(0.0) as u64;
            out.current_temperature = number(get("current_temperature"));
            out.target_temperature = if features & 1 != 0 {
                number(get("temperature"))
            } else {
                None
            };
            out.target_temperature_low = if features & 2 != 0 {
                number(get("target_temp_low"))
            } else {
                None
            };
            out.target_temperature_high = if features & 2 != 0 {
                number(get("target_temp_high"))
            } else {
                None
            };
            out.min_temperature = number(get("min_temp"));
            out.max_temperature = number(get("max_temp"));
            out.temperature_step = number(get("target_temp_step"));
            out.temperature_unit = text(get("temperature_unit"));
            out.hvac_modes = texts(get("hvac_modes"));
            out.hvac_action = text(get("hvac_action"));
            out.preset_mode = text(get("preset_mode"));
            out.preset_modes = texts(get("preset_modes"));
            out.fan_mode = text(get("fan_mode"));
            out.fan_modes = texts(get("fan_modes"));
            out.current_humidity = number(get("current_humidity"));
            out.target_humidity = number(get("humidity"));
            out.min_humidity = number(get("min_humidity"));
            out.max_humidity = number(get("max_humidity"));
        }
        "water_heater" => {
            out.current_temperature = number(get("current_temperature"));
            out.target_temperature = number(get("temperature"));
            out.min_temperature = number(get("min_temp"));
            out.max_temperature = number(get("max_temp"));
            out.mode = text(get("operation_mode"));
            out.modes = texts(get("operation_list"));
        }
        "humidifier" => {
            out.current_humidity = number(get("current_humidity"));
            out.target_humidity = number(get("humidity"));
            out.min_humidity = number(get("min_humidity"));
            out.max_humidity = number(get("max_humidity"));
            out.mode = text(get("mode"));
            out.modes = texts(get("available_modes"));
            out.hvac_action = text(get("action"));
        }
        "cover" | "valve" => {
            out.position = number(get("current_position"));
            out.tilt_position = number(get("current_tilt_position"));
        }
        "fan" => {
            out.percentage = number(get("percentage"));
            out.percentage_step = number(get("percentage_step"));
            out.oscillating = boolean(get("oscillating"));
            out.direction = text(get("direction"));
            out.preset_mode = text(get("preset_mode"));
            out.preset_modes = texts(get("preset_modes"));
        }
        "media_player" => {
            out.media_title = text(get("media_title"));
            out.media_artist =
                text(get("media_artist")).or_else(|| text(get("media_album_artist")));
            out.media_album = text(get("media_album_name"));
            out.media_series_title = text(get("media_series_title"));
            out.media_content_type = text(get("media_content_type"));
            out.media_source = text(get("source"));
            out.sources = texts(get("source_list"));
            out.app_name = text(get("app_name"));
            out.volume = number(get("volume_level"));
            out.is_muted = boolean(get("is_volume_muted"));
            out.media_duration = number(get("media_duration"));
            out.media_position = number(get("media_position"));
            out.media_position_updated_at = timestamp(get("media_position_updated_at"));
            out.shuffle = boolean(get("shuffle"));
            out.repeat = text(get("repeat"));
        }
        "weather" => {
            out.current_temperature = number(get("temperature"));
            out.temperature_unit = text(get("temperature_unit"));
            out.humidity = number(get("humidity"));
            out.pressure = number(get("pressure"));
            out.wind_speed = number(get("wind_speed"));
            out.wind_speed_unit = text(get("wind_speed_unit"));
        }
        "update" => {
            out.installed_version = text(get("installed_version"));
            out.latest_version = text(get("latest_version"));
            out.release_summary = text(get("release_summary"));
            out.release_url = text(get("release_url"));
            out.in_progress = boolean(get("in_progress"));
        }
        "automation" | "script" => {
            out.last_triggered = timestamp(get("last_triggered"));
            out.current_run_count = number(get("current"));
        }
        "alarm_control_panel" => {
            out.code_required = boolean(get("code_arm_required"));
            out.code_format = text(get("code_format"));
        }
        "lock" => out.code_format = text(get("code_format")),
        "vacuum" | "lawn_mower" => {
            out.status = text(get("status"));
            out.fan_speed = text(get("fan_speed"));
            out.fan_speeds = texts(get("fan_speed_list"));
        }
        "input_select" | "select" => out.options = texts(get("options")),
        "input_number" | "number" => {
            out.min = number(get("min"));
            out.max = number(get("max"));
            out.step = number(get("step"));
            out.mode = text(get("mode"));
        }
        "counter" => {
            out.min = number(get("minimum"));
            out.max = number(get("maximum"));
            out.step = number(get("step"));
        }
        "sun" => {
            out.next_setting = timestamp(get("next_setting"));
            out.next_rising = timestamp(get("next_rising"));
        }
        "timer" => {
            out.remaining = text(get("remaining"));
            out.duration = text(get("duration"));
            out.finishes_at = timestamp(get("finishes_at"));
        }
        "calendar" => {
            out.message = text(get("message"));
            out.start_time = timestamp(get("start_time"));
            out.end_time = timestamp(get("end_time"));
            out.location = text(get("location"));
        }
        _ => {}
    }
    out
}

pub fn get_entity_domain(entity_id: &str) -> &str {
    entity_id.split('.').next().unwrap_or(entity_id)
}

#[derive(Debug, Clone, Default)]
pub struct EntityRegistryContext {
    pub area_id: Option<String>,
    pub registry_name: Option<String>,
    pub registry_icon: Option<String>,
    pub hidden: bool,
}

pub fn normalize_entity(raw: &RawEntityState, context: &EntityRegistryContext) -> SmartHomeEntity {
    let domain = get_entity_domain(&raw.entity_id).to_string();
    let attributes = &raw.attributes;
    let mut common = domain_attributes(&domain, attributes);
    common.supported_features =
        number(attributes.get("supported_features")).map(|value| value.max(0.0) as u64);
    common.battery_level = number(attributes.get("battery_level"));
    SmartHomeEntity {
        id: raw.entity_id.clone(),
        name: text(attributes.get("friendly_name"))
            .or_else(|| context.registry_name.clone())
            .unwrap_or_else(|| raw.entity_id.clone()),
        domain,
        area_id: context.area_id.clone(),
        state: raw.state.clone(),
        icon: text(attributes.get("icon")).or_else(|| context.registry_icon.clone()),
        device_class: text(attributes.get("device_class")),
        state_class: text(attributes.get("state_class")),
        unit: text(attributes.get("unit_of_measurement")),
        last_changed: raw.last_changed,
        last_updated: raw.last_updated,
        context_id: raw.context_id.clone(),
        attributes: common,
    }
}

pub struct RegistryLookup {
    entries: HashMap<String, EntityRegistryContext>,
}

impl RegistryLookup {
    pub fn new(registry: Option<&HomeAssistantRegistry>) -> Self {
        let Some(registry) = registry else {
            return Self {
                entries: HashMap::new(),
            };
        };
        let device_areas: HashMap<&str, Option<&String>> = registry
            .devices
            .iter()
            .map(|device| (device.id.as_str(), device.area_id.as_ref()))
            .collect();
        let entries = registry
            .entities
            .entities
            .iter()
            .map(|entry| {
                let area = entry.ai.clone().or_else(|| {
                    entry
                        .di
                        .as_deref()
                        .and_then(|device| device_areas.get(device).copied().flatten().cloned())
                });
                (
                    entry.ei.clone(),
                    EntityRegistryContext {
                        area_id: area,
                        registry_name: entry.en.clone(),
                        registry_icon: entry.ic.clone(),
                        hidden: entry.hb == Some(true) || entry.ec.is_some(),
                    },
                )
            })
            .collect();
        Self { entries }
    }

    pub fn get(&self, entity_id: &str) -> EntityRegistryContext {
        self.entries.get(entity_id).cloned().unwrap_or_default()
    }
}

pub fn normalize_areas(registry: Option<&HomeAssistantRegistry>) -> Vec<SmartHomeArea> {
    registry
        .map(|registry| {
            registry
                .areas
                .iter()
                .map(|area| SmartHomeArea {
                    id: area.area_id.clone(),
                    name: area.name.clone(),
                    floor_id: area.floor_id.clone(),
                    icon: area.icon.clone(),
                    temperature_entity_id: area.temperature_entity_id.clone(),
                    humidity_entity_id: area.humidity_entity_id.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn normalize_floors(registry: Option<&HomeAssistantRegistry>) -> Vec<SmartHomeFloor> {
    registry
        .map(|registry| {
            registry
                .floors
                .iter()
                .map(|floor| SmartHomeFloor {
                    id: floor.floor_id.clone(),
                    name: floor.name.clone(),
                    level: floor.level,
                    icon: floor.icon.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn create_catalog<'a>(
    states: impl IntoIterator<Item = &'a RawEntityState>,
    registry: Option<&HomeAssistantRegistry>,
) -> SmartHomeCatalog {
    let lookup = RegistryLookup::new(registry);
    let mut entities: Vec<SmartHomeCatalogEntity> = states
        .into_iter()
        .map(|raw| {
            let context = lookup.get(&raw.entity_id);
            let entity = normalize_entity(raw, &context);
            SmartHomeCatalogEntity {
                id: entity.id,
                domain: entity.domain,
                name: entity.name,
                area_id: entity.area_id,
                icon: entity.icon,
                device_class: entity.device_class,
                hidden: context.hidden,
            }
        })
        .collect();
    entities.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    SmartHomeCatalog {
        entities,
        areas: normalize_areas(registry),
        floors: normalize_floors(registry),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::groups::home_misc::homeassistant::types::{
        AreaEntry, DeviceEntry, EntityDisplayRegistry, EntityRegistryEntry,
    };

    fn raw(entity_id: &str, state: &str, attributes: Value) -> RawEntityState {
        RawEntityState {
            entity_id: entity_id.into(),
            state: state.into(),
            attributes: attributes.as_object().cloned().unwrap_or_default(),
            last_changed: 1.0,
            last_updated: 2.0,
            context_id: None,
        }
    }

    #[test]
    fn normalizes_lights_by_supported_color_modes() {
        let entity = normalize_entity(
            &raw(
                "light.kitchen",
                "on",
                json!({"friendly_name": "Kitchen", "brightness": 128, "supported_color_modes": ["color_temp", "xy"],
                       "color_temp_kelvin": 3000, "rgb_color": [255, 0, 10], "supported_features": 4}),
            ),
            &EntityRegistryContext::default(),
        );
        assert_eq!(entity.name, "Kitchen");
        assert_eq!(entity.attributes.brightness, Some(50.0));
        assert_eq!(entity.attributes.color_temp_kelvin, Some(3000.0));
        assert_eq!(entity.attributes.rgb_color, Some([255, 0, 10]));
        assert_eq!(entity.attributes.supported_features, Some(4));

        let dimmer = normalize_entity(
            &raw(
                "light.d",
                "on",
                json!({"brightness": 255, "supported_color_modes": ["onoff"]}),
            ),
            &EntityRegistryContext::default(),
        );
        assert_eq!(dimmer.attributes.brightness, None);
    }

    #[test]
    fn climate_targets_depend_on_features() {
        let entity = normalize_entity(
            &raw(
                "climate.living",
                "heat_cool",
                json!({"supported_features": 3, "temperature": 21, "target_temp_low": 19, "target_temp_high": 24}),
            ),
            &EntityRegistryContext::default(),
        );
        assert_eq!(entity.attributes.target_temperature, Some(21.0));
        assert_eq!(entity.attributes.target_temperature_low, Some(19.0));
        let range_only = normalize_entity(
            &raw(
                "climate.x",
                "heat",
                json!({"supported_features": 1, "temperature": 20, "target_temp_low": 1}),
            ),
            &EntityRegistryContext::default(),
        );
        assert_eq!(range_only.attributes.target_temperature_low, None);
    }

    #[test]
    fn registry_lookup_resolves_areas_names_and_hidden() {
        let registry = HomeAssistantRegistry {
            areas: vec![AreaEntry {
                area_id: "kitchen".into(),
                name: "Kitchen".into(),
                floor_id: None,
                icon: None,
                temperature_entity_id: None,
                humidity_entity_id: None,
            }],
            floors: vec![],
            devices: vec![DeviceEntry {
                id: "dev1".into(),
                area_id: Some("kitchen".into()),
            }],
            entities: EntityDisplayRegistry {
                entities: vec![
                    EntityRegistryEntry {
                        ei: "light.a".into(),
                        ai: None,
                        di: Some("dev1".into()),
                        en: Some("Lamp".into()),
                        ic: Some("mdi:lamp".into()),
                        ec: None,
                        hb: None,
                    },
                    EntityRegistryEntry {
                        ei: "sensor.b".into(),
                        ai: Some("office".into()),
                        di: Some("dev1".into()),
                        en: None,
                        ic: None,
                        ec: Some(1.0),
                        hb: None,
                    },
                ],
            },
        };
        let lookup = RegistryLookup::new(Some(&registry));
        let a = lookup.get("light.a");
        assert_eq!(a.area_id.as_deref(), Some("kitchen"));
        assert!(!a.hidden);
        let b = lookup.get("sensor.b");
        assert_eq!(b.area_id.as_deref(), Some("office"));
        assert!(b.hidden);
        assert!(lookup.get("unknown.x").area_id.is_none());

        let entity = normalize_entity(&raw("light.a", "on", json!({})), &a);
        assert_eq!(
            (entity.name.as_str(), entity.icon.as_deref()),
            ("Lamp", Some("mdi:lamp"))
        );

        let catalog = create_catalog(
            [
                &raw("light.a", "on", json!({})),
                &raw("sensor.b", "1", json!({"friendly_name": "alpha"})),
            ],
            Some(&registry),
        );
        assert_eq!(catalog.entities[0].name, "alpha");
        assert!(!catalog.entities[1].hidden);
    }
}
