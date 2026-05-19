use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::benchmark::{self, BenchmarkRun};
use crate::config::AppConfig;
use crate::db::{Db, FileSnapshot};
use crate::models::{IndexStats, ParsedNote};
use crate::{graph, paths, tantivy_index, vault, vector_search};

/// Options controlling a local index refresh.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct IndexOptions {
    pub rebuild: bool,
    pub changed_only: bool,
    pub no_embeddings: bool,
}

/// Summary returned after refreshing the local index.
#[derive(Debug, Clone, Serialize)]
pub struct IndexOutcome {
    pub stats: IndexStats,
    pub graph_warnings: usize,
    pub embeddings: usize,
}

/// Rebuilds the local SQLite and Tantivy indexes, reusing unchanged embeddings.
pub fn refresh_with_benchmark(
    config: &AppConfig,
    options: IndexOptions,
    mut benchmark: Option<&mut BenchmarkRun>,
) -> Result<IndexOutcome> {
    let mut parsed = benchmark::time_phase(&mut benchmark, "load_vault_ms", || {
        vault::load_vault(config)
    })?;
    let graph_report = benchmark::time_phase(&mut benchmark, "resolve_graph_ms", || {
        graph::resolve_links(&mut parsed)
    });
    let paths = paths::KbPaths::from_config(config);
    if options.rebuild {
        benchmark::time_phase(&mut benchmark, "reset_indexes_ms", || {
            reset_local_indexes(&paths)
        })?;
    }
    let mut db = benchmark::time_phase(&mut benchmark, "open_db_ms", || Db::open(&paths.db_path))?;
    let previous_files =
        benchmark::time_phase(&mut benchmark, "file_snapshots_ms", || db.file_snapshots())?;
    let mut stats = benchmark::time_phase(&mut benchmark, "sqlite_replace_ms", || {
        db.replace_index(&parsed, &graph_report.warnings)
    })?;
    apply_change_stats(&mut stats, &previous_files, &parsed);
    let chunks = benchmark::time_phase(&mut benchmark, "load_chunks_ms", || db.load_all_chunks())?;
    benchmark::time_phase(&mut benchmark, "tantivy_rebuild_ms", || {
        tantivy_index::rebuild(&paths.tantivy_dir, &chunks, config.index.remove_diacritics)
    })?;
    let embeddings = if options.no_embeddings || !config.embeddings.enabled {
        0
    } else {
        benchmark::time_phase(&mut benchmark, "embeddings_rebuild_ms", || {
            vector_search::rebuild_embeddings(&db, config)
        })?
    };

    Ok(IndexOutcome {
        stats,
        graph_warnings: graph_report.warnings.len(),
        embeddings,
    })
}

fn reset_local_indexes(paths: &paths::KbPaths) -> Result<()> {
    if paths.db_path.exists() {
        std::fs::remove_file(&paths.db_path)
            .with_context(|| format!("failed to delete {}", paths.db_path.display()))?;
    }
    if paths.tantivy_dir.exists() {
        std::fs::remove_dir_all(&paths.tantivy_dir)
            .with_context(|| format!("failed to delete {}", paths.tantivy_dir.display()))?;
    }
    Ok(())
}

fn apply_change_stats(
    stats: &mut IndexStats,
    previous: &BTreeMap<String, FileSnapshot>,
    current: &[ParsedNote],
) {
    let current_paths = current
        .iter()
        .map(|note| note.path.clone())
        .collect::<BTreeSet<_>>();
    for note in current {
        let current_snapshot = FileSnapshot {
            mtime_ns: note.mtime,
            size_bytes: note.size as i64,
            content_hash: note.hash.clone(),
        };
        if previous.get(&note.path) == Some(&current_snapshot) {
            stats.unchanged_files += 1;
        } else {
            stats.changed_files += 1;
        }
    }
    stats.deleted_files = previous
        .keys()
        .filter(|path| !current_paths.contains(*path))
        .count();
}
