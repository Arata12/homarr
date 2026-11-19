use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

use crate::error::{HomarrError, HomarrResult};

/// Default serialized empty SuperJSON payload used in Homarr v2 database columns.
pub const EMPTY_SUPERJSON: &str = r#"{"json":{}}"#;

/// Serializes a value in SuperJSON-compatible `{"json": ...}` format.
pub fn serialize_superjson<T: Serialize>(value: &T) -> HomarrResult<String> {
    let inner = serde_json::to_value(value).map_err(|err| {
        HomarrError::Internal(format!("Failed to serialize superjson value: {err}"))
    })?;
    serde_json::to_string(&json!({ "json": inner }))
        .map_err(|err| HomarrError::Internal(format!("Failed to encode superjson string: {err}")))
}

/// Deserializes a SuperJSON string (`{"json": ...}` with optional `"meta"`) or falls back to raw JSON.
pub fn deserialize_superjson<T: DeserializeOwned>(raw: &str) -> HomarrResult<T> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::from_value(Value::Object(Default::default())).map_err(|err| {
            HomarrError::BadRequest(format!("Failed to deserialize empty superjson: {err}"))
        });
    }

    let parsed: Value = serde_json::from_str(trimmed).map_err(|err| {
        HomarrError::BadRequest(format!("Invalid JSON in superjson column: {err}"))
    })?;

    let payload = if let Some(obj) = parsed.as_object() {
        if let Some(inner) = obj.get("json") {
            inner.clone()
        } else {
            parsed
        }
    } else {
        parsed
    };

    serde_json::from_value(payload).map_err(|err| {
        HomarrError::BadRequest(format!("Failed to decode superjson payload: {err}"))
    })
}

/// Extracts the inner `serde_json::Value` from a SuperJSON or plain JSON string.
pub fn parse_superjson_value(raw: &str) -> Value {
    deserialize_superjson::<Value>(raw).unwrap_or_else(|_| Value::Object(Default::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct DummyOptions {
        title: String,
        #[serde(default)]
        count: u32,
    }

    #[test]
    fn superjson_round_trip_and_raw_fallback() {
        let opts = DummyOptions {
            title: "Links".to_string(),
            count: 3,
        };
        let encoded = serialize_superjson(&opts).unwrap();
        assert_eq!(encoded, r#"{"json":{"count":3,"title":"Links"}}"#);
        let decoded: DummyOptions = deserialize_superjson(&encoded).unwrap();
        assert_eq!(decoded, opts);

        // SuperJSON with meta field from Node superjson
        let with_meta =
            r#"{"json":{"title":"FromNode","count":7},"meta":{"values":{"createdAt":["Date"]}}}"#;
        let decoded_meta: DummyOptions = deserialize_superjson(with_meta).unwrap();
        assert_eq!(decoded_meta.title, "FromNode");
        assert_eq!(decoded_meta.count, 7);

        // Plain JSON fallback
        let raw = r#"{"title":"Plain","count":1}"#;
        let decoded_raw: DummyOptions = deserialize_superjson(raw).unwrap();
        assert_eq!(decoded_raw.title, "Plain");
    }
}
