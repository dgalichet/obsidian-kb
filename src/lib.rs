#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod chunking;
pub mod cli;
pub mod config;
pub mod db;
pub mod doctor;
pub mod embeddings;
pub mod error;
pub mod frontmatter;
pub mod graph;
pub mod markdown;
pub mod models;
pub mod normalization;
pub mod output;
pub mod paths;
pub mod schema;
pub mod scoring;
pub mod search;
pub mod tantivy_index;
pub mod vault;
pub mod vector_search;
pub mod version;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Command};
use db::FileSnapshot;
use models::SearchMode;
use std::collections::{BTreeMap, BTreeSet};

pub fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .without_time()
        .try_init()
        .ok();

    let cli = Cli::parse();
    let global_config = cli.config.clone();
    match cli.command {
        Command::Init(args) => {
            let vault = args
                .vault
                .as_deref()
                .or(args.legacy_vault.as_deref())
                .context("missing vault; use `obsidian-kb init --vault /path/to/vault`")?;
            let config = config::AppConfig::default_for_vault(vault, args.index_dir.as_deref())?;
            config::save_config(&config)?;
            let paths = paths::KbPaths::from_config(&config);
            db::Db::open(&paths.db_path)?;
            println!("initialized {}", paths.index_dir.display());
        }
        Command::Index(args) => {
            let mut config =
                config::load_or_default(args.vault.as_deref(), global_config.as_deref())?;
            if let Some(vault) = args.vault.as_deref() {
                config.vault.path = std::fs::canonicalize(vault)?;
            }
            config::save_config(&config)?;

            let mut parsed = vault::load_vault(&config)?;
            let graph_report = graph::resolve_links(&mut parsed);
            let paths = paths::KbPaths::from_config(&config);
            if args.rebuild {
                reset_local_indexes(&paths)?;
            }
            if args.changed_only {
                eprintln!(
                    "warning: --changed-only is deprecated; use `obsidian-kb index`. Regular indexing refreshes SQLite/Tantivy and reuses unchanged embeddings."
                );
            }
            let mut db = db::Db::open(&paths.db_path)?;
            let previous_files = db.file_snapshots()?;
            let mut stats = db.replace_index(&parsed, &graph_report.warnings)?;
            apply_change_stats(&mut stats, &previous_files, &parsed);
            let chunks = db.load_all_chunks()?;
            tantivy_index::rebuild(&paths.tantivy_dir, &chunks, config.index.remove_diacritics)?;
            let embeddings = if args.no_embeddings || !config.embeddings.enabled {
                0
            } else {
                vector_search::rebuild_embeddings(&db, &config)?
            };
            output::print_index_summary(&stats, graph_report.warnings.len(), embeddings);
        }
        Command::Search(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let query = args.query.join(" ");
            let mode = args
                .mode
                .map(SearchMode::from)
                .unwrap_or_else(|| config.default_search_mode());
            let hits = search::search(
                &config,
                &query,
                mode,
                args.top,
                args.expand_graph,
                args.include_text,
                args.max_chars,
            )?;
            if args.json {
                output::print_json(&hits)?;
            } else {
                output::print_search_table(&hits);
            }
        }
        Command::Show(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let paths = paths::KbPaths::from_config(&config);
            let db = db::Db::open(&paths.db_path)?;
            let Some(chunk) = db.load_chunk(&args.chunk_id)? else {
                bail!("chunk not found: {}", args.chunk_id);
            };
            if args.json {
                output::print_json(&chunk)?;
            } else {
                output::print_chunk(&chunk);
            }
        }
        Command::Graph(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let paths = paths::KbPaths::from_config(&config);
            let db = db::Db::open(&paths.db_path)?;
            let Some(view) = db.graph_view(&args.note, args.depth)? else {
                bail!("note not found: {}", args.note);
            };
            if args.json {
                output::print_json(&view)?;
            } else {
                output::print_graph_view(&view);
            }
        }
        Command::Stats(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let paths = paths::KbPaths::from_config(&config);
            let db = db::Db::open(&paths.db_path)?;
            let report = db.stats()?;
            if args.json {
                output::print_json(&report)?;
            } else {
                output::print_stats(&report);
            }
        }
        Command::Doctor(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let report = doctor::run(&config)?;
            if args.json {
                output::print_json(&report)?;
            } else {
                output::print_doctor_report(&report);
            }
            if report.fatal_count > 0 {
                std::process::exit(2);
            }
        }
        Command::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), version::version());
        }
    }

    Ok(())
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
    stats: &mut models::IndexStats,
    previous: &BTreeMap<String, FileSnapshot>,
    current: &[models::ParsedNote],
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
