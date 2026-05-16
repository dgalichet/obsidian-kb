use serde_json::Value;
use std::collections::BTreeSet;

use crate::config::PropertyIndexConfig;
use crate::models::PropertyRecord;

pub fn extract(metadata: &Value, config: &PropertyIndexConfig) -> Vec<PropertyRecord> {
    if !config.enabled {
        return Vec::new();
    }
    let Some(object) = metadata.as_object() else {
        return Vec::new();
    };

    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, value) in object {
        let key_norm = normalize_key(key);
        if key_norm.is_empty() || !key_allowed(&key_norm, config) {
            continue;
        }
        for (value_text, value_type, value_json) in property_values(value) {
            if value_text.is_empty() || too_long(&value_text, config.max_value_chars) {
                continue;
            }
            let value_norm = normalize_value(&value_text);
            if value_norm.is_empty() {
                continue;
            }
            if seen.insert((key_norm.clone(), value_norm.clone())) {
                records.push(PropertyRecord {
                    key: key_norm.clone(),
                    value_text,
                    value_norm,
                    value_type,
                    value_json,
                });
            }
        }
    }
    records
}

pub fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub fn normalize_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn key_allowed(key: &str, config: &PropertyIndexConfig) -> bool {
    if matches_any(&config.ignored_keys, key) {
        return false;
    }
    matches_any(&config.filter_keys, key)
}

fn matches_any(patterns: &[String], key: &str) -> bool {
    patterns
        .iter()
        .map(|pattern| pattern.trim().to_ascii_lowercase())
        .any(|pattern| pattern_matches(&pattern, key))
}

fn pattern_matches(pattern: &str, key: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return key.starts_with(prefix);
    }
    pattern == key
}

fn property_values(value: &Value) -> Vec<(String, String, Value)> {
    match value {
        Value::String(value) => trimmed(value)
            .map(|value| (value.clone(), "string".to_string(), Value::String(value)))
            .into_iter()
            .collect(),
        Value::Bool(value) => vec![(
            value.to_string(),
            "boolean".to_string(),
            Value::Bool(*value),
        )],
        Value::Number(value) => vec![(
            value.to_string(),
            "number".to_string(),
            Value::Number(value.clone()),
        )],
        Value::Array(values) => values
            .iter()
            .flat_map(property_values)
            .map(|(value_text, value_type, value_json)| {
                (value_text, format!("array:{value_type}"), value_json)
            })
            .collect(),
        Value::Null | Value::Object(_) => Vec::new(),
    }
}

fn trimmed(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn too_long(value: &str, max_chars: usize) -> bool {
    max_chars > 0 && value.chars().count() > max_chars
}
