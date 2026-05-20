use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::db::Db;
use crate::embeddings::FastEmbedder;
use crate::markdown;
use crate::models::{
    DoctorCheck, DoctorIssue, DoctorLinkGroup, DoctorReport, UnresolvedLinkRecord,
};
use crate::normalization;
use crate::paths::KbPaths;
use crate::version;
use crate::{tantivy_index, vault};

struct DoctorCounts {
    markdown_files: usize,
    pdf_files: usize,
    indexed_files: usize,
    chunks: usize,
    embeddings: usize,
}

struct LinkClassification {
    level: &'static str,
    category: &'static str,
    suggestion: &'static str,
}

struct ClassifiedUnresolvedLink {
    target: String,
    category: String,
    file: String,
}

pub fn run(config: &AppConfig) -> Result<DoctorReport> {
    let paths = KbPaths::from_config(config);
    let version = version::version().to_string();
    let mut checks = Vec::new();
    let mut issues = Vec::new();

    check(
        &mut checks,
        &mut issues,
        "config file",
        paths.config_path.exists(),
        format!("{}", paths.config_path.display()),
        true,
    );
    check(
        &mut checks,
        &mut issues,
        "vault path",
        paths.vault_path.is_dir(),
        format!("{}", paths.vault_path.display()),
        true,
    );

    let exclude_ok = vault::validate_exclude_globs(&config.vault.exclude_globs).is_ok();
    check(
        &mut checks,
        &mut issues,
        "exclude globs",
        exclude_ok,
        format!("{} patterns", config.vault.exclude_globs.len()),
        true,
    );

    let doctor_ignore_globs = build_globset(&config.doctor.unresolved_links.ignore_globs);
    check(
        &mut checks,
        &mut issues,
        "doctor unresolved link ignore globs",
        doctor_ignore_globs.is_ok(),
        format!(
            "{} patterns",
            config.doctor.unresolved_links.ignore_globs.len()
        ),
        false,
    );
    let doctor_ignore_globs = doctor_ignore_globs.ok();

    let markdown_files = vault::count_markdown_files(config).unwrap_or(0);
    checks.push(DoctorCheck {
        name: "markdown files".to_string(),
        status: "ok".to_string(),
        message: markdown_files.to_string(),
    });
    let pdf_files = vault::count_pdf_files(config).unwrap_or(0);
    checks.push(DoctorCheck {
        name: "pdf files".to_string(),
        status: "ok".to_string(),
        message: pdf_files.to_string(),
    });

    let index_dir_ok = std::fs::create_dir_all(&paths.index_dir).is_ok();
    check(
        &mut checks,
        &mut issues,
        "index directory",
        index_dir_ok,
        format!("{}", paths.index_dir.display()),
        true,
    );

    let db_result = Db::open(&paths.db_path);
    check(
        &mut checks,
        &mut issues,
        "sqlite database",
        db_result.is_ok(),
        format!("{}", paths.db_path.display()),
        true,
    );

    let tantivy_result = tantivy_index::open_or_create(&paths.tantivy_dir);
    check(
        &mut checks,
        &mut issues,
        "tantivy index",
        tantivy_result.is_ok(),
        format!("{}", paths.tantivy_dir.display()),
        true,
    );

    let embedding_ok = if config.embeddings.enabled {
        FastEmbedder::new(config).is_ok()
    } else {
        true
    };
    check(
        &mut checks,
        &mut issues,
        "embedding model",
        embedding_ok,
        if config.embeddings.enabled {
            config.embedding_model_key()
        } else {
            "disabled".to_string()
        },
        false,
    );

    let mut notes = 0;
    let mut chunks = 0;
    let mut embeddings = 0;
    let mut classified_unresolved_links = Vec::new();
    if let Ok(db) = db_result {
        let stats = db.stats()?;
        notes = stats.notes;
        chunks = stats.chunks;
        embeddings = stats.embeddings;
        checks.push(DoctorCheck {
            name: "indexed files".to_string(),
            status: "ok".to_string(),
            message: stats.notes.to_string(),
        });
        checks.push(DoctorCheck {
            name: "chunks".to_string(),
            status: "ok".to_string(),
            message: stats.chunks.to_string(),
        });
        checks.push(DoctorCheck {
            name: "embeddings".to_string(),
            status: "ok".to_string(),
            message: stats.embeddings.to_string(),
        });

        for (alias, paths) in db.duplicate_aliases()? {
            issues.push(DoctorIssue {
                level: "warning".to_string(),
                category: "duplicate_alias".to_string(),
                message: format!("duplicate alias `{alias}` in {}", paths.join(", ")),
                file: None,
                target: Some(alias),
                raw: None,
                link_type: Some("alias".to_string()),
                suggestion: Some(
                    "Rename one alias or make the intended target explicit.".to_string(),
                ),
            });
        }
        for link in db.unresolved_link_records()? {
            let target = link_target(&link);
            let classification =
                classify_unresolved_link(config, doctor_ignore_globs.as_ref(), &link, &target);
            issues.push(DoctorIssue {
                level: classification.level.to_string(),
                category: classification.category.to_string(),
                message: format!(
                    "{} has {} unresolved link {}",
                    link.source_path, link.link_type, link.target_raw
                ),
                file: Some(link.source_path.clone()),
                target: Some(target.clone()),
                raw: Some(link.target_raw.clone()),
                link_type: Some(link.link_type.clone()),
                suggestion: Some(classification.suggestion.to_string()),
            });
            classified_unresolved_links.push(ClassifiedUnresolvedLink {
                target,
                category: classification.category.to_string(),
                file: link.source_path,
            });
        }
        for warning in db.warnings()? {
            if is_redundant_graph_warning(&warning) {
                continue;
            }
            issues.push(DoctorIssue {
                level: "warning".to_string(),
                category: "index_warning".to_string(),
                message: warning,
                file: None,
                target: None,
                raw: None,
                link_type: None,
                suggestion: None,
            });
        }
    }

    dedupe_issues(&mut issues);
    let unresolved_link_groups = group_unresolved_links(&classified_unresolved_links);

    Ok(report(
        version,
        paths,
        DoctorCounts {
            markdown_files,
            pdf_files,
            indexed_files: notes,
            chunks,
            embeddings,
        },
        checks,
        unresolved_link_groups,
        issues,
    ))
}

fn check(
    checks: &mut Vec<DoctorCheck>,
    issues: &mut Vec<DoctorIssue>,
    name: &str,
    ok: bool,
    message: String,
    fatal: bool,
) {
    checks.push(DoctorCheck {
        name: name.to_string(),
        status: if ok { "ok" } else { "failed" }.to_string(),
        message: message.clone(),
    });
    if !ok {
        issues.push(DoctorIssue {
            level: if fatal { "fatal" } else { "warning" }.to_string(),
            category: "check_failed".to_string(),
            message: format!("{name} check failed: {message}"),
            file: None,
            target: None,
            raw: None,
            link_type: None,
            suggestion: None,
        });
    }
}

fn dedupe_issues(issues: &mut Vec<DoctorIssue>) {
    let mut seen = BTreeSet::new();
    issues.retain(|issue| {
        seen.insert((
            issue.level.clone(),
            issue.category.clone(),
            issue.message.clone(),
            issue.file.clone(),
            issue.target.clone(),
        ))
    });
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn classify_unresolved_link(
    config: &AppConfig,
    ignore_globs: Option<&GlobSet>,
    link: &UnresolvedLinkRecord,
    target: &str,
) -> LinkClassification {
    if ignored_target(config, target) || ignored_source(ignore_globs, &link.source_path) {
        return LinkClassification {
            level: "info",
            category: "ignored_unresolved_link",
            suggestion: "This unresolved link matches the configured doctor ignore policy.",
        };
    }

    if generated_noise_source(&link.source_path) {
        return LinkClassification {
            level: "info",
            category: "generated_noise",
            suggestion: "Consider adding this folder to doctor.unresolved_links.ignore_globs.",
        };
    }

    if existing_asset_embed(
        config.vault_path(),
        &link.source_path,
        target,
        &link.link_type,
    ) {
        return LinkClassification {
            level: "info",
            category: "asset_embed",
            suggestion: "The embedded asset exists on disk but is not an indexed Markdown note.",
        };
    }

    if link.link_type == "embedded" {
        return LinkClassification {
            level: "warning",
            category: "missing_asset_embed",
            suggestion: "Check the embedded asset path or add the containing folder to the vault.",
        };
    }

    if is_forward_link(target) && config.doctor.unresolved_links.allow_forward_links {
        return LinkClassification {
            level: "info",
            category: "forward_link",
            suggestion: "Forward links are allowed by doctor.unresolved_links.allow_forward_links.",
        };
    }

    if is_forward_link(target) {
        return LinkClassification {
            level: "warning",
            category: "possible_forward_link",
            suggestion: "Create the note, add an alias, or enable allow_forward_links if this is intentional.",
        };
    }

    LinkClassification {
        level: "warning",
        category: "broken_link",
        suggestion: "Check the target path, casing, extension, or Obsidian alias.",
    }
}

fn ignored_target(config: &AppConfig, target: &str) -> bool {
    let target = normalize_for_policy(target);
    config
        .doctor
        .unresolved_links
        .ignore_targets
        .iter()
        .any(|ignored| normalize_for_policy(ignored) == target)
}

fn ignored_source(ignore_globs: Option<&GlobSet>, source_path: &str) -> bool {
    ignore_globs
        .map(|globs| globs.is_match(source_path))
        .unwrap_or(false)
}

fn generated_noise_source(source_path: &str) -> bool {
    let source = source_path.to_ascii_lowercase();
    source.contains("readwise")
        || source.contains("ultimate-todoist-sync/reports")
        || source.contains("/reports/")
        || source.starts_with("reports/")
}

fn existing_asset_embed(
    vault_path: &Path,
    source_path: &str,
    target: &str,
    link_type: &str,
) -> bool {
    if link_type != "embedded" {
        return false;
    }

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

fn is_forward_link(target: &str) -> bool {
    let target = target.trim();
    !target.is_empty()
        && !target.contains('/')
        && !target.contains('\\')
        && Path::new(target).extension().is_none()
}

fn link_target(link: &UnresolvedLinkRecord) -> String {
    markdown::extract_wikilinks(&link.target_raw)
        .into_iter()
        .next()
        .map(|link| link.target)
        .unwrap_or_else(|| link.target_normalized.clone())
}

fn normalize_for_policy(value: &str) -> String {
    let normalized = normalization::normalize_text(value, true)
        .trim()
        .to_ascii_lowercase();
    normalized.trim_end_matches(".md").to_string()
}

fn is_redundant_graph_warning(warning: &str) -> bool {
    warning.contains(": unresolved link `") || warning.contains("duplicate alias `")
}

fn group_unresolved_links(links: &[ClassifiedUnresolvedLink]) -> Vec<DoctorLinkGroup> {
    let mut groups = BTreeMap::<(String, String), (usize, BTreeSet<String>)>::new();
    for link in links {
        let (occurrences, files) = groups
            .entry((link.target.clone(), link.category.clone()))
            .or_default();
        *occurrences += 1;
        files.insert(link.file.clone());
    }

    let mut groups = groups
        .into_iter()
        .map(
            |((target, category), (occurrences, files))| DoctorLinkGroup {
                target,
                category,
                occurrences,
                files: files.into_iter().collect(),
            },
        )
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.target.cmp(&right.target))
    });
    groups
}

fn report(
    version: String,
    paths: KbPaths,
    counts: DoctorCounts,
    checks: Vec<DoctorCheck>,
    unresolved_link_groups: Vec<DoctorLinkGroup>,
    issues: Vec<DoctorIssue>,
) -> DoctorReport {
    let fatal_count = issues.iter().filter(|issue| issue.level == "fatal").count();
    let warning_count = issues
        .iter()
        .filter(|issue| issue.level == "warning")
        .count();
    let info_count = issues.iter().filter(|issue| issue.level == "info").count();
    DoctorReport {
        version,
        config_path: paths.config_path.display().to_string(),
        vault_path: paths.vault_path.display().to_string(),
        index_dir: paths.index_dir.display().to_string(),
        markdown_files: counts.markdown_files,
        pdf_files: counts.pdf_files,
        indexed_files: counts.indexed_files,
        embeddings: counts.embeddings,
        issue_count: issues.len(),
        fatal_count,
        warning_count,
        info_count,
        notes: counts.indexed_files,
        chunks: counts.chunks,
        checks,
        unresolved_link_groups,
        issues,
    }
}
