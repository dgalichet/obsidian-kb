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
pub mod indexer;
pub mod markdown;
pub mod mcp_server;
pub mod models;
pub mod normalization;
pub mod output;
pub mod paths;
pub mod properties;
pub mod related;
pub mod schema;
pub mod scoring;
pub mod search;
pub mod serve;
pub mod tantivy_index;
pub mod vault;
pub mod vector_search;
pub mod version;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cli::{Cli, Command};
use models::{SearchFilters, SearchMode};
use std::io::Read;

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
                    if args.changed_only {
                        eprintln!(
                            "warning: --changed-only is deprecated; use `obsidian-kb index`. Regular indexing refreshes SQLite/Tantivy and reuses unchanged embeddings."
                        );
                    }
                    let outcome = indexer::refresh_with_benchmark(
                        &config,
                        indexer::IndexOptions {
                            rebuild: args.rebuild,
                            changed_only: args.changed_only,
                            no_embeddings: args.no_embeddings,
                        },
                        benchmark.as_deref_mut(),
                    )?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || {
                        output::print_index_summary(
                            &outcome.stats,
                            outcome.graph_warnings,
                            outcome.embeddings,
                        );
                    });
                    Ok(())
                },
            )?;
        }
        Command::Search(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let query = args.query.join(" ");
            let filters = SearchFilters {
                tags: args.tags.clone(),
                properties: args
                    .properties
                    .iter()
                    .map(|value| search::parse_property_filter(value))
                    .collect::<Result<Vec<_>>>()?,
            };
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
                    benchmark.set_field("tag_filters", filters.tags.len());
                    benchmark.set_field("property_filters", filters.properties.len());
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
                            filters: filters.clone(),
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
        Command::Related(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            let input = related_input(&args)?;
            benchmark::measure(
                &config,
                "related",
                |benchmark| {
                    benchmark.set_field("top", args.top);
                    benchmark.set_field("candidates", args.candidates);
                    benchmark.set_field("json", args.json);
                    match &input {
                        related::RelatedInput::Note(identifier) => {
                            benchmark.set_field("source_kind", "note");
                            benchmark
                                .set_field("source_identifier_chars", identifier.chars().count());
                            if config.benchmark.include_query {
                                benchmark.set_field("source_identifier", identifier);
                            }
                        }
                        related::RelatedInput::Text(text) => {
                            benchmark.set_field("source_kind", "text");
                            benchmark.set_field("source_text_chars", text.chars().count());
                            if config.benchmark.include_query {
                                benchmark.set_field("source_text", text);
                            }
                        }
                    }
                },
                |mut benchmark| {
                    let report = related::find_related_with_benchmark(
                        &config,
                        input.clone(),
                        related::RelatedOptions {
                            limit: args.top,
                            candidates: args.candidates,
                        },
                        benchmark.as_deref_mut(),
                    )?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&report)?;
                        } else {
                            output::print_related_table(&report);
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
        Command::Tags(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            benchmark::measure(
                &config,
                "tags",
                |benchmark| {
                    benchmark.set_field("top", args.top);
                    benchmark.set_field("json", args.json);
                    if let Some(prefix) = args.prefix.as_deref() {
                        benchmark.set_field("prefix", prefix);
                    }
                },
                |mut benchmark| {
                    let paths = paths::KbPaths::from_config(&config);
                    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let tags = benchmark::time_phase(&mut benchmark, "tags_ms", || {
                        db.tag_facets(args.prefix.as_deref(), args.top)
                    })?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&tags)?;
                        } else {
                            output::print_tags(&tags);
                        }
                        Ok(())
                    })?;
                    Ok(())
                },
            )?;
        }
        Command::Properties(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            benchmark::measure(
                &config,
                "properties",
                |benchmark| {
                    benchmark.set_field("top", args.top);
                    benchmark.set_field("json", args.json);
                    if let Some(key) = args.key.as_deref() {
                        benchmark.set_field("key", key);
                    }
                },
                |mut benchmark| {
                    let paths = paths::KbPaths::from_config(&config);
                    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || {
                        db::Db::open(&paths.db_path)
                    })?;
                    let report = benchmark::time_phase(&mut benchmark, "properties_ms", || {
                        db.property_facets(args.key.as_deref(), args.top)
                    })?;
                    benchmark::time_phase(&mut benchmark, "output_ms", || -> Result<()> {
                        if args.json {
                            output::print_json(&report)?;
                        } else {
                            output::print_properties(&report);
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
        Command::Serve(args) => {
            let config = config::load_existing(args.vault.as_deref(), global_config.as_deref())?;
            serve::run(config, args)?;
        }
        Command::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), version::version());
        }
    }

    Ok(())
}

fn related_input(args: &cli::RelatedArgs) -> Result<related::RelatedInput> {
    let note = args.note.join(" ");
    let has_note = !note.trim().is_empty();
    let source_count = if has_note { 1 } else { 0 }
        + if args.text.is_some() { 1 } else { 0 }
        + if args.stdin { 1 } else { 0 };
    if source_count != 1 {
        bail!("related requires exactly one source: NOTE, --text, or --stdin");
    }
    if let Some(text) = args.text.as_ref() {
        return Ok(related::RelatedInput::Text(text.clone()));
    }
    if args.stdin {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        return Ok(related::RelatedInput::Text(text));
    }
    Ok(related::RelatedInput::Note(note))
}

fn search_mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Bm25 => "bm25",
        SearchMode::Vector => "vector",
        SearchMode::Hybrid => "hybrid",
    }
}
