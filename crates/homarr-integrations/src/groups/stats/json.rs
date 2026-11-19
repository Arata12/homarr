//! Small validation helpers standing in for the zod schemas of the TS providers.
//!
//! All helpers are pure and return `Result<_, String>`; callers map the message through
//! `error::parse_error`. Numbers follow the zod shapes used by the providers: `count` is a finite,
//! non-negative integer, `nonneg` a finite, non-negative number.

use serde_json::{Map, Value};

use crate::capabilities::stats::StatsValue;

fn describe(path: &[&str]) -> String {
    path.join(".")
}

/// Walks `path`; a missing key or a non-object parent is an error.
pub fn at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Value, String> {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .ok_or_else(|| format!("missing field '{}'", describe(path)))?;
    }
    Ok(current)
}

/// Like [`at`] but a missing or `null` field is `None`.
pub fn opt<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    (!current.is_null()).then_some(current)
}

pub fn as_nonneg(value: &Value, what: &str) -> Result<f64, String> {
    match value.as_f64() {
        Some(number) if number.is_finite() && number >= 0.0 => Ok(number),
        _ => Err(format!("'{what}' must be a non-negative number")),
    }
}

pub fn as_count(value: &Value, what: &str) -> Result<f64, String> {
    let number = as_nonneg(value, what)?;
    if number.fract() == 0.0 {
        Ok(number)
    } else {
        Err(format!("'{what}' must be a non-negative integer"))
    }
}

/// Finite, non-negative integer at `path`.
pub fn count(value: &Value, path: &[&str]) -> Result<f64, String> {
    as_count(at(value, path)?, &describe(path))
}

/// Finite, non-negative number at `path`.
pub fn nonneg(value: &Value, path: &[&str]) -> Result<f64, String> {
    as_nonneg(at(value, path)?, &describe(path))
}

pub fn text<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, String> {
    at(value, path)?
        .as_str()
        .ok_or_else(|| format!("'{}' must be a string", describe(path)))
}

pub fn boolean(value: &Value, path: &[&str]) -> Result<bool, String> {
    at(value, path)?
        .as_bool()
        .ok_or_else(|| format!("'{}' must be a boolean", describe(path)))
}

pub fn array<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Vec<Value>, String> {
    at(value, path)?
        .as_array()
        .ok_or_else(|| format!("'{}' must be an array", describe(path)))
}

pub fn object<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Map<String, Value>, String> {
    at(value, path)?
        .as_object()
        .ok_or_else(|| format!("'{}' must be an object", describe(path)))
}

pub fn num(value: f64) -> StatsValue {
    StatsValue::Number(value)
}

pub fn len(value: usize) -> StatsValue {
    StatsValue::Number(value as f64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn count_rejects_negative_fractional_and_missing() {
        let v = json!({ "a": 3, "b": -1, "c": 1.5, "d": 4.0 });
        assert_eq!(count(&v, &["a"]), Ok(3.0));
        assert!(count(&v, &["b"]).is_err());
        assert!(count(&v, &["c"]).is_err());
        assert_eq!(count(&v, &["d"]), Ok(4.0));
        assert!(count(&v, &["zzz"]).is_err());
    }

    #[test]
    fn opt_treats_null_as_missing() {
        let v = json!({ "a": null, "b": { "c": 1 } });
        assert!(opt(&v, &["a"]).is_none());
        assert!(opt(&v, &["b", "c"]).is_some());
    }
}
