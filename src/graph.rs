use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::models::{GraphReport, ParsedNote};
use crate::normalization;

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
        let note_path = note.path.clone();
        let note_vault_path = vault_path_for_note(note);
        for link in note.links.iter_mut() {
            let keys = link_keys(&note_path, &link.target);
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
                                note_path, anchor, target_path
                            ));
                        }
                    }
                }
                Some(paths) => {
                    report.warnings.push(format!(
                        "{}: ambiguous link `{}` could match {}",
                        note_path,
                        link.raw,
                        paths.join(", ")
                    ));
                }
                None => {
                    if existing_embed_asset(
                        note_vault_path.as_deref(),
                        &note_path,
                        &link.target,
                        link.embedded,
                    ) {
                        continue;
                    }
                    report
                        .warnings
                        .push(format!("{}: unresolved link `{}`", note_path, link.raw));
                }
            }
        }
    }

    report
}

fn normalize_target(value: &str) -> String {
    let normalized = normalization::normalize_text(value, true)
        .replace('\\', "/")
        .trim()
        .trim_start_matches("./")
        .to_lowercase();
    normalized.trim_end_matches(".md").to_string()
}

fn existing_embed_asset(
    vault_path: Option<&Path>,
    source_path: &str,
    target: &str,
    embedded: bool,
) -> bool {
    if !embedded {
        return false;
    }
    let Some(vault_path) = vault_path else {
        return false;
    };
    asset_candidates(vault_path, source_path, target)
        .into_iter()
        .any(|candidate| {
            if !candidate.is_file() {
                return false;
            }
            let path = candidate.to_string_lossy();
            candidate.extension().and_then(|ext| ext.to_str()) != Some("md")
                || path.ends_with(".excalidraw.md")
        })
}

fn asset_candidates(vault_path: &Path, source_path: &str, target: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let normalized_target = target.replace('\\', "/");
    let target_path = Path::new(&normalized_target);

    if target_path.is_absolute() {
        candidates.push(target_path.to_path_buf());
    } else {
        candidates.push(vault_path.join(target_path));
        if normalized_target.starts_with("./") || normalized_target.starts_with("../") {
            let source_parent = Path::new(source_path).parent().unwrap_or(Path::new(""));
            candidates.push(vault_path.join(source_parent).join(target_path));
        }
    }

    let mut with_md = Vec::new();
    for candidate in &candidates {
        if candidate.extension().and_then(|ext| ext.to_str()) != Some("md") {
            with_md.push(PathBuf::from(format!("{}.md", candidate.to_string_lossy())));
        }
    }
    candidates.extend(with_md);
    candidates
}

fn vault_path_for_note(note: &ParsedNote) -> Option<PathBuf> {
    let mut path = note.absolute_path.clone();
    for _ in Path::new(&note.path).components() {
        path.pop();
    }
    path.is_dir().then_some(path)
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
