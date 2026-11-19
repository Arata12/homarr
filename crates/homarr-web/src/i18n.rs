use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::http::{header::COOKIE, HeaderMap};
use serde_json::Value;

#[derive(Clone, Default)]
pub struct I18nCatalog {
    locales_dir: PathBuf,
    sorted_codes: Vec<String>,
    cache: Arc<RwLock<HashMap<String, Arc<Value>>>>,
}

impl I18nCatalog {
    /// Scans `locales_dir` for `*.json` file stems without parsing any translation files yet.
    /// Individual locale files are loaded and parsed lazily on first request.
    pub fn load(locales_dir: &Path) -> Self {
        let mut sorted_codes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(locales_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if !stem.is_empty() {
                        sorted_codes.push(stem.to_string());
                    }
                }
            }
        }
        sorted_codes.sort();
        sorted_codes.dedup();
        Self {
            locales_dir: locales_dir.to_path_buf(),
            sorted_codes,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn codes(&self) -> &[String] {
        &self.sorted_codes
    }

    pub fn len(&self) -> usize {
        self.sorted_codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sorted_codes.is_empty()
    }

    pub fn cached_count(&self) -> usize {
        self.cache.read().map(|m| m.len()).unwrap_or(0)
    }

    fn has_locale(&self, code: &str) -> bool {
        self.sorted_codes
            .binary_search_by(|c| c.as_str().cmp(code))
            .is_ok()
    }

    fn get_locale(&self, code: &str) -> Option<Arc<Value>> {
        if !self.has_locale(code) {
            return None;
        }
        if let Ok(guard) = self.cache.read() {
            if let Some(existing) = guard.get(code) {
                return Some(Arc::clone(existing));
            }
        }
        let path = self.locales_dir.join(format!("{code}.json"));
        let raw = std::fs::read_to_string(path).ok()?;
        let parsed = Arc::new(serde_json::from_str::<Value>(&raw).ok()?);
        if let Ok(mut guard) = self.cache.write() {
            let entry = guard
                .entry(code.to_string())
                .or_insert_with(|| Arc::clone(&parsed));
            return Some(Arc::clone(entry));
        }
        Some(parsed)
    }

    pub fn negotiate(&self, headers: &HeaderMap) -> String {
        if let Some(cookie_hdr) = headers.get(COOKIE).and_then(|v| v.to_str().ok()) {
            for part in cookie_hdr.split(';') {
                if let Some((k, v)) = part.trim().split_once('=') {
                    if k.trim() == "homarr-locale" {
                        let candidate = v.trim();
                        if self.has_locale(candidate) {
                            return candidate.to_string();
                        }
                    }
                }
            }
        }

        if let Some(accept) = headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
        {
            for item in accept.split(',') {
                let tag = item.split(';').next().unwrap_or("").trim();
                if self.has_locale(tag) {
                    return tag.to_string();
                }
                let base = tag.split('-').next().unwrap_or("");
                if self.has_locale(base) {
                    return base.to_string();
                }
            }
        }

        "en".to_string()
    }

    pub fn t(&self, locale: &str, dotted_key: &str, fallback: &str) -> String {
        if let Some(root) = self.get_locale(locale) {
            if let Some(val) = lookup_dotted(root.as_ref(), dotted_key) {
                return val.to_string();
            }
        }
        if locale != "en" {
            if let Some(root) = self.get_locale("en") {
                if let Some(val) = lookup_dotted(root.as_ref(), dotted_key) {
                    return val.to_string();
                }
            }
        }
        fallback.to_string()
    }
}

fn lookup_dotted<'a>(mut curr: &'a Value, path: &str) -> Option<&'a str> {
    for part in path.split('.') {
        curr = curr.as_object()?.get(part)?;
    }
    curr.as_str()
}
