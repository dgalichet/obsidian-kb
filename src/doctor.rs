use anyhow::Result;

use crate::config::AppConfig;
use crate::db::Db;
use crate::embeddings::FastEmbedder;
use crate::models::{DoctorCheck, DoctorIssue, DoctorReport};
use crate::paths::KbPaths;
use crate::{tantivy_index, vault};
use std::collections::BTreeSet;

struct DoctorCounts {
    markdown_files: usize,
    indexed_files: usize,
    chunks: usize,
    embeddings: usize,
}

pub fn run(config: &AppConfig) -> Result<DoctorReport> {
    let paths = KbPaths::from_config(config);
    let version = env!("CARGO_PKG_VERSION").to_string();
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

    let markdown_files = vault::count_markdown_files(config).unwrap_or(0);
    checks.push(DoctorCheck {
        name: "markdown files".to_string(),
        status: "ok".to_string(),
        message: markdown_files.to_string(),
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
                message: format!("duplicate alias `{alias}` in {}", paths.join(", ")),
            });
        }
        for (path, raw) in db.unresolved_links()? {
            issues.push(DoctorIssue {
                level: "warning".to_string(),
                message: format!("{path} has unresolved link {raw}"),
            });
        }
        for warning in db.warnings()? {
            issues.push(DoctorIssue {
                level: "warning".to_string(),
                message: warning,
            });
        }
    }

    dedupe_issues(&mut issues);

    Ok(report(
        version,
        paths,
        DoctorCounts {
            markdown_files,
            indexed_files: notes,
            chunks,
            embeddings,
        },
        checks,
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
            message: format!("{name} check failed: {message}"),
        });
    }
}

fn dedupe_issues(issues: &mut Vec<DoctorIssue>) {
    let mut seen = BTreeSet::new();
    issues.retain(|issue| seen.insert((issue.level.clone(), issue.message.clone())));
}

fn report(
    version: String,
    paths: KbPaths,
    counts: DoctorCounts,
    checks: Vec<DoctorCheck>,
    issues: Vec<DoctorIssue>,
) -> DoctorReport {
    let fatal_count = issues.iter().filter(|issue| issue.level == "fatal").count();
    let warning_count = issues
        .iter()
        .filter(|issue| issue.level == "warning")
        .count();
    DoctorReport {
        version,
        config_path: paths.config_path.display().to_string(),
        vault_path: paths.vault_path.display().to_string(),
        index_dir: paths.index_dir.display().to_string(),
        markdown_files: counts.markdown_files,
        indexed_files: counts.indexed_files,
        embeddings: counts.embeddings,
        fatal_count,
        warning_count,
        notes: counts.indexed_files,
        chunks: counts.chunks,
        checks,
        issues,
    }
}
