use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

use crate::models::{Heading, WikiLink};

pub fn title_from(metadata_title: Option<String>, body: &str, path: &str) -> String {
    if let Some(title) = metadata_title {
        return title;
    }
    if let Some(heading) = extract_headings(body).into_iter().find(|h| h.level == 1) {
        return heading.text;
    }
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace('-', " ")
}

pub fn extract_wikilinks(markdown: &str) -> Vec<WikiLink> {
    let stripped = strip_fenced_code(markdown);
    wikilink_regex()
        .captures_iter(&stripped)
        .filter_map(|capture| {
            let raw = capture.get(0)?.as_str().to_string();
            let embedded = capture.get(1).is_some();
            let inner = capture.get(2)?.as_str().trim();
            if inner.is_empty() {
                return None;
            }
            let (target_part, display) = split_once_trimmed(inner, '|');
            let (target, anchor) = split_once_trimmed(target_part, '#');
            if target.is_empty() {
                return None;
            }
            Some(WikiLink {
                raw,
                target: target.to_string(),
                anchor: anchor.filter(|value| !value.is_empty()).map(str::to_string),
                display: display
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                embedded,
                target_path: None,
            })
        })
        .collect()
}

pub fn extract_tags(markdown: &str) -> Vec<String> {
    let stripped = strip_fenced_code(markdown);
    let mut tags = BTreeSet::new();
    for capture in tag_regex().captures_iter(&stripped) {
        if let Some(tag) = capture.get(2) {
            let value = tag.as_str().trim_matches('/').to_string();
            if !value.is_empty() && !value.chars().all(|ch| ch.is_ascii_digit()) {
                tags.insert(value);
            }
        }
    }
    tags.into_iter().collect()
}

pub fn extract_headings(markdown: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut in_code = false;
    for (index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some(capture) = heading_regex().captures(line) {
            let level = capture.get(1).map(|m| m.as_str().len()).unwrap_or(1);
            let text = capture
                .get(2)
                .map(|m| m.as_str().trim().trim_matches('#').trim().to_string())
                .unwrap_or_default();
            if !text.is_empty() {
                headings.push(Heading {
                    level,
                    slug: slugify(&text),
                    text,
                    line: index + 1,
                });
            }
        }
    }
    headings
}

pub fn strip_fenced_code(markdown: &str) -> String {
    let mut stripped = String::with_capacity(markdown.len());
    let mut in_code = false;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            stripped.push('\n');
            continue;
        }
        if in_code {
            stripped.push('\n');
        } else {
            stripped.push_str(line);
        }
    }
    stripped
}

pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn split_once_trimmed(value: &str, delimiter: char) -> (&str, Option<&str>) {
    if let Some((left, right)) = value.split_once(delimiter) {
        (left.trim(), Some(right.trim()))
    } else {
        (value.trim(), None)
    }
}

fn wikilink_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(!)?\[\[([^\]]+)\]\]").expect("valid wikilink regex"))
}

fn tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(^|[\s\(\[{>])#([A-Za-z][A-Za-z0-9_/-]*)").expect("valid tag regex")
    })
}

fn heading_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").expect("valid heading regex"))
}
