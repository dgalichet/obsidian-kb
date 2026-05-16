use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct FrontmatterParse {
    pub metadata: Value,
    pub body: String,
    pub body_start_line: usize,
    pub warning: Option<String>,
}

pub fn parse(content: &str) -> FrontmatterParse {
    let Some(first_line) = content.split_inclusive('\n').next() else {
        return empty(content);
    };
    if first_line.trim_end_matches(['\r', '\n']) != "---" {
        return empty(content);
    }

    let mut offset = first_line.len();
    for line in content[first_line.len()..].split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let yaml = &content[first_line.len()..offset];
            let body_start = offset + line.len();
            let body = content.get(body_start..).unwrap_or_default().to_string();
            let body_start_line = line_number_at(content, body_start);
            return match serde_yaml::from_str::<serde_yaml::Value>(yaml) {
                Ok(value) => FrontmatterParse {
                    metadata: yaml_to_json(value),
                    body,
                    body_start_line,
                    warning: None,
                },
                Err(error) => FrontmatterParse {
                    metadata: Value::Object(Map::new()),
                    body,
                    body_start_line,
                    warning: Some(format!("invalid frontmatter: {error}")),
                },
            };
        }
        offset += line.len();
    }

    FrontmatterParse {
        metadata: Value::Object(Map::new()),
        body: content.to_string(),
        body_start_line: 1,
        warning: Some("unterminated frontmatter block".to_string()),
    }
}

pub fn string_field(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn string_list_field(metadata: &Value, key: &str) -> Vec<String> {
    let Some(value) = metadata.get(key) else {
        return Vec::new();
    };
    match value {
        Value::String(value) => split_tag_like(value),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split_tag_like)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn string_values_field(metadata: &Value, key: &str) -> Vec<String> {
    let Some(value) = metadata.get(key) else {
        return Vec::new();
    };
    match value {
        Value::String(value) => trimmed_string(value).into_iter().collect(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .filter_map(trimmed_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn empty(content: &str) -> FrontmatterParse {
    FrontmatterParse {
        metadata: Value::Object(Map::new()),
        body: content.to_string(),
        body_start_line: 1,
        warning: None,
    }
}

fn line_number_at(content: &str, byte_offset: usize) -> usize {
    content
        .get(..byte_offset)
        .unwrap_or(content)
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn trimmed_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn split_tag_like(value: &str) -> Vec<String> {
    value
        .split([',', ' '])
        .map(|item| item.trim().trim_start_matches('#'))
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn yaml_to_json(value: serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(value) => Value::Bool(value),
        serde_yaml::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::Number(value.into())
            } else if let Some(value) = value.as_u64() {
                Value::Number(value.into())
            } else if let Some(value) = value.as_f64() {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(value) => Value::String(value),
        serde_yaml::Value::Sequence(values) => {
            Value::Array(values.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(mapping) => {
            let mut object = Map::new();
            for (key, value) in mapping {
                let key = match key {
                    serde_yaml::Value::String(key) => key,
                    other => serde_yaml::to_string(&other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                object.insert(key, yaml_to_json(value));
            }
            Value::Object(object)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}
