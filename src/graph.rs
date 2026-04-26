use std::collections::{BTreeMap, BTreeSet};

use crate::models::{GraphReport, ParsedNote};

pub fn resolve_links(notes: &mut [ParsedNote]) -> GraphReport {
    let mut report = GraphReport::default();
    let mut path_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stem_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut alias_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut title_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut heading_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for note in notes.iter() {
        path_map
            .entry(normalize_target(note.path.trim_end_matches(".md")))
            .or_default()
            .push(note.path.clone());
        let stem = note
            .path
            .rsplit_once('/')
            .map(|(_, stem)| stem)
            .unwrap_or(&note.path)
            .trim_end_matches(".md");
        stem_map
            .entry(normalize_target(stem))
            .or_default()
            .push(note.path.clone());
        title_map
            .entry(normalize_target(&note.title))
            .or_default()
            .push(note.path.clone());
        heading_map.insert(
            note.path.clone(),
            note.headings
                .iter()
                .flat_map(|heading| [heading.slug.clone(), normalize_target(&heading.text)])
                .collect(),
        );
        for alias in &note.aliases {
            alias_map
                .entry(normalize_target(alias))
                .or_default()
                .push(note.path.clone());
        }
    }

    for (alias, paths) in &alias_map {
        let unique = paths.iter().collect::<BTreeSet<_>>();
        if unique.len() > 1 {
            report.warnings.push(format!(
                "duplicate alias `{alias}` used by {}",
                unique.into_iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    for note in notes.iter_mut() {
        for link in note.links.iter_mut() {
            let keys = link_keys(&note.path, &link.target);
            let candidates = if link.target.contains('/') {
                find_candidates(&path_map, &keys)
            } else {
                find_candidates(&stem_map, &keys)
                    .or_else(|| find_candidates(&alias_map, &keys))
                    .or_else(|| find_candidates(&title_map, &keys))
                    .or_else(|| find_candidates(&path_map, &keys))
            };
            match candidates {
                Some(paths) if paths.len() == 1 => {
                    link.target_path = paths.first().cloned();
                    if let (Some(anchor), Some(target_path)) = (&link.anchor, &link.target_path) {
                        let anchor_key = normalize_target(anchor);
                        if !heading_map
                            .get(target_path)
                            .map(|headings| headings.contains(&anchor_key))
                            .unwrap_or(false)
                        {
                            report.warnings.push(format!(
                                "{}: missing heading `{}` in `{}`",
                                note.path, anchor, target_path
                            ));
                        }
                    }
                }
                Some(paths) => {
                    report.warnings.push(format!(
                        "{}: ambiguous link `{}` could match {}",
                        note.path,
                        link.raw,
                        paths.join(", ")
                    ));
                }
                None => {
                    report
                        .warnings
                        .push(format!("{}: unresolved link `{}`", note.path, link.raw));
                }
            }
        }
    }

    report
}

fn normalize_target(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim()
        .trim_start_matches("./")
        .trim_end_matches(".md")
        .to_lowercase()
}

fn find_candidates<'a>(
    map: &'a BTreeMap<String, Vec<String>>,
    keys: &[String],
) -> Option<&'a Vec<String>> {
    keys.iter().find_map(|key| map.get(key))
}

fn link_keys(source_path: &str, target: &str) -> Vec<String> {
    let target = target.trim().trim_end_matches(".md");
    let mut keys = vec![normalize_target(target)];
    if target.starts_with("./") || target.starts_with("../") {
        let parent = source_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        keys.push(normalize_target(&join_relative(parent, target)));
    }
    keys.sort();
    keys.dedup();
    keys
}

fn join_relative(parent: &str, target: &str) -> String {
    let mut parts = parent
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}
