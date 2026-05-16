#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod benchmark;
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
pub mod mcp_server;
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

            benchmark::measure(
                &config,
                "index",
                |benchmark| {
                    benchmark.set_field("rebuild", args.rebuild);
                    benchmark.set_field("changed_only", args.changed_only);
                    benchmark.set_field("no_embeddings", args.no_embeddings);
                },
                |mut benchmark| {
                    let mut parsed =
                        benchmark::time_phase(&mut benchmark, "load_vault_ms", || {
                            vault::load_vault(&config)
                        })?;
                    let graph_report =
                        benchmark::time_phase(&mut benchmark, "resolve_graph_ms", || {
                            graph::resolve_links(&mut parsed)
                        });
                    let paths = paths::KbPaths::from_config(&config);
                    if args.rebuild {
                        benchmark::time_phase(&mut benchmark, "reset_indexes_ms", || {
                            reset_local_indexes(&paths)
                        })?;
                    }
                    if args.changed_only {
                        eprintln!(
                            "warning: --changed-only is deprecated; use `obsidian-kb index`. Regular indexing refreshes SQLite/Tantivy and reuses unchanged embeddings."
                        );
                    }
                    let mut db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let previous_files =
                        benchmark::time_phase(&mut benchmark, "file_snapshots_ms", || {
                            db.file_snapshots()
                        })?;
                    let mut stats =
                        benchmark::time_phase(&mut benchmark, "sqlite_replace_ms", || {
                            db.replace_index(&parsed, &graph_report.warnings)
                        })?;
                    apply_change_stats(&mut stats, &previous_files, &parsed);
                    let chunks = benchmark::time_phase(&mut benchmark, "load_chunks_ms", || {
                        db.load_all_chunks()
                    })?;
                    benchmark::time_phase(&mut benchmark, "tantivy_rebuild_ms", || {
                        tantivy_index::rebuild(
                            &paths.tantivy_dir,
                            &chunks,
                            config.index.remove_diacritics,
                        )
                    })?;
                    let embeddings = if args.no_embeddings || !config.embeddings.enabled {
                        0
                    } else {
                        benchmark::time_phase(&mut benchmark, "embeddings_rebuild_ms", || {
                            vector_search::rebuild_embeddings(&db, &config)
                        })?
                    };
                    benchmark::time_phase(&mut benchmark, "output_ms", || {
                        output::print_index_summary(
                            &stats,
                            graph_report.warnings.len(),
                            embeddings,
                        );
                    });
                    Ok(())
                },
            )?;
        }
        Command::Search(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let query = args.query.join(" ");
            let mode = args
                .mode
                .map(SearchMode::from)
                .unwrap_or_else(|| config.default_search_mode());
            benchmark::measure(
                &config,
                "search",
                |benchmark| {
                    benchmark.set_field("mode", search_mode_name(mode));
                    benchmark.set_field("top", args.top);
                    benchmark.set_field("expand_graph", args.expand_graph);
                    benchmark.set_field("include_text", args.include_text);
                    benchmark.set_field("max_chars", args.max_chars);
                    benchmark.set_field("json", args.json);
                    benchmark.set_field("query_chars", query.chars().count());
                    if config.benchmark.include_query {
                        benchmark.set_field("query", &query);
                    }
                },
                |mut benchmark| {
                    let hits = search::search_with_benchmark(
                        &config,
                        &query,
                        search::SearchOptions {
                            mode,
                            limit: args.top,
                            graph: args.expand_graph,
                            include_text: args.include_text,
                            max_chars: args.max_chars,
                        },
                        benchmark.as_deref_mut(),
                    )?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&hits)?;
                        } else {
                            output::print_search_table(&hits);
                        }
                        Ok(())
                    })?;
                    Ok(())
                },
            )?;
        }
        Command::Show(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            benchmark::measure(
                &config,
                "show",
                |benchmark| {
                    benchmark.set_field("json", args.json);
                },
                |mut benchmark| {
                    let paths = paths::KbPaths::from_config(&config);
                    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let Some(chunk) =
                        benchmark::time_phase(&mut benchmark, "load_chunk_ms", || {
                            db.load_chunk(&args.chunk_id)
                        })?
                    else {
                        bail!("chunk not found: {}", args.chunk_id);
                    };
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&chunk)?;
                        } else {
                            output::print_chunk(&chunk);
                        }
                        Ok(())
                    })?;
                    Ok(())
                },
            )?;
        }
        Command::Graph(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            benchmark::measure(
                &config,
                "graph",
                |benchmark| {
                    benchmark.set_field("depth", args.depth);
                    benchmark.set_field("json", args.json);
                },
                |mut benchmark| {
                    let paths = paths::KbPaths::from_config(&config);
                    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let Some(view) =
                        benchmark::time_phase(&mut benchmark, "graph_view_ms", || {
                            db.graph_view(&args.note, args.depth)
                        })?
                    else {
                        bail!("note not found: {}", args.note);
                    };
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&view)?;
                        } else {
                            output::print_graph_view(&view);
                        }
                        Ok(())
                    })?;
                    Ok(())
                },
            )?;
        }
        Command::Stats(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            benchmark::measure(
                &config,
                "stats",
                |benchmark| {
                    benchmark.set_field("json", args.json);
                },
                |mut benchmark| {
                    let paths = paths::KbPaths::from_config(&config);
                    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let report = benchmark::time_phase(&mut benchmark, "stats_ms", || db.stats())?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&report)?;
                        } else {
                            output::print_stats(&report);
                        }
                        Ok(())
                    })?;
                    Ok(())
                },
            )?;
        }
        Command::Doctor(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let fatal_count = benchmark::measure(
                &config,
                "doctor",
                |benchmark| {
                    benchmark.set_field("json", args.json);
                },
                |mut benchmark| {
                    let report = benchmark::time_phase(&mut benchmark, "doctor_run_ms", || {
                        doctor::run(&config)
                    })?;
                    let fatal_count = report.fatal_count;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&report)?;
                        } else {
                            output::print_doctor_report(&report);
                        }
                        Ok(())
                    })?;
                    Ok(fatal_count)
                },
            )?;
            if fatal_count > 0 {
                std::process::exit(2);
            }
        }
        Command::Mcp(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            mcp_server::run(config, args)?;
        }
        Command::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), version::version());
        }
    }

    Ok(())
}

fn search_mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Bm25 => "bm25",
        SearchMode::Vector => "vector",
        SearchMode::Hybrid => "hybrid",
    }
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
