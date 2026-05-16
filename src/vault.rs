use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::chunking;
use crate::config::AppConfig;
use crate::frontmatter;
use crate::markdown;
use crate::models::ParsedNote;
use crate::properties;

pub fn load_vault(config: &AppConfig) -> Result<Vec<ParsedNote>> {
    let markdown_paths = markdown_paths(config)?;
    let mut notes = Vec::with_capacity(markdown_paths.len());
    for (path, relative_string) in markdown_paths {
        notes.push(parse_note_file(
            config.vault_path(),
            &path,
            &relative_string,
            config,
        )?);
    }

    notes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(notes)
}

pub fn count_markdown_files(config: &AppConfig) -> Result<usize> {
    Ok(markdown_paths(config)?.len())
}

pub fn validate_exclude_globs(patterns: &[String]) -> Result<()> {
    build_excludes(patterns).map(|_| ())
}

fn markdown_paths(config: &AppConfig) -> Result<Vec<(PathBuf, String)>> {
    let excludes = build_excludes(&config.vault.exclude_globs)?;
    let mut paths = Vec::new();
    let walker = WalkBuilder::new(config.vault_path())
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let relative = path.strip_prefix(config.vault_path())?;
        let relative_string = normalize_relative_path(relative);
        if excludes.is_match(&relative_string) {
            continue;
        }
        paths.push((path.to_path_buf(), relative_string));
    }
    paths.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(paths)
}

pub fn parse_note_file(
    vault_path: &Path,
    absolute_path: &Path,
    relative_path: &str,
    config: &AppConfig,
) -> Result<ParsedNote> {
    let content = std::fs::read_to_string(absolute_path)
        .with_context(|| format!("failed to read note: {}", absolute_path.display()))?;
    let metadata = std::fs::metadata(absolute_path)?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or_default();
    parse_note_content(
        vault_path,
        absolute_path.to_path_buf(),
        relative_path,
        &content,
        mtime,
        metadata.len(),
        config,
    )
}

pub fn parse_note_content(
    _vault_path: &Path,
    absolute_path: PathBuf,
    relative_path: &str,
    content: &str,
    mtime: i64,
    size: u64,
    config: &AppConfig,
) -> Result<ParsedNote> {
    let parsed_frontmatter = frontmatter::parse(content);
    let mut warnings = Vec::new();
    if let Some(warning) = parsed_frontmatter.warning {
        warnings.push(format!("{relative_path}: {warning}"));
    }
    let mut aliases = frontmatter::string_values_field(&parsed_frontmatter.metadata, "aliases");
    aliases.extend(frontmatter::string_values_field(
        &parsed_frontmatter.metadata,
        "alias",
    ));
    let aliases = normalize_strings(aliases);
    let mut tags = BTreeSet::new();
    for tag in frontmatter::string_list_field(&parsed_frontmatter.metadata, "tags") {
        tags.insert(clean_tag(&tag));
    }
    for tag in markdown::extract_tags(&parsed_frontmatter.body) {
        tags.insert(clean_tag(&tag));
    }
    let tags = tags
        .into_iter()
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    let properties = properties::extract(&parsed_frontmatter.metadata, &config.index.properties);
    let metadata_title = frontmatter::string_field(&parsed_frontmatter.metadata, "title");
    let title = markdown::title_from(metadata_title, &parsed_frontmatter.body, relative_path);
    let body_line_offset = parsed_frontmatter.body_start_line.saturating_sub(1);
    let headings = markdown::extract_headings(&parsed_frontmatter.body)
        .into_iter()
        .map(|mut heading| {
            heading.line += body_line_offset;
            heading
        })
        .collect::<Vec<_>>();
    let links = markdown::extract_wikilinks(&parsed_frontmatter.body);
    let chunks = chunking::chunk_markdown_with_body_start_line(
        relative_path,
        &title,
        &tags,
        &parsed_frontmatter.body,
        parsed_frontmatter.body_start_line,
        &headings,
        &config.index,
    );
    let folder = Path::new(relative_path)
        .parent()
        .map(normalize_relative_path)
        .unwrap_or_default();

    Ok(ParsedNote {
        path: relative_path.to_string(),
        absolute_path,
        title,
        folder,
        hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
        mtime,
        size,
        frontmatter: parsed_frontmatter.metadata,
        body: parsed_frontmatter.body,
        aliases,
        tags,
        properties,
        links,
        headings,
        chunks,
        warnings,
    })
}

pub fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn build_excludes(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn normalize_strings(values: Vec<String>) -> Vec<String> {
    let mut set = BTreeSet::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() {
            set.insert(value.to_string());
        }
    }
    set.into_iter().collect()
}

fn clean_tag(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .trim_matches('/')
        .to_string()
}
