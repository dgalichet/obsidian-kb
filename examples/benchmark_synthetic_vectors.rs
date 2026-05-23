use anyhow::{Context, Result, bail};
use obsidian_kb::{
    benchmark, config, db::Db, paths::KbPaths, synthetic::deterministic_unit_vector,
    vector_search::VectorSearchCache,
};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut config_path = None;
    let mut runs = 3usize;
    let mut limit = 80usize;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = Some(PathBuf::from(
                    args.next().context("missing value after --config")?,
                ));
            }
            "--runs" => {
                runs = args
                    .next()
                    .context("missing value after --runs")?
                    .parse()
                    .context("--runs must be a positive integer")?;
            }
            "--limit" => {
                limit = args
                    .next()
                    .context("missing value after --limit")?
                    .parse()
                    .context("--limit must be a positive integer")?;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => bail!("unknown argument `{other}`"),
        }
    }

    if runs == 0 {
        bail!("--runs must be greater than 0");
    }

    let config =
        config::load_existing(None, config_path.as_deref()).context("failed to load config")?;
    let paths = KbPaths::from_config(&config);
    let db = Db::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    let model_key = config.embedding_model_key();
    let query_vector = deterministic_unit_vector(
        "synthetic benchmark query vector",
        config.embedding_dimensions(),
    );
    let mut cache = VectorSearchCache::default();

    for run in 1..=runs {
        benchmark::measure(
            &config,
            "synthetic_vector_search",
            |benchmark| {
                benchmark.set_field("run", run);
                benchmark.set_field("limit", limit);
                benchmark.set_field("query_vector", "deterministic");
            },
            |benchmark| {
                let hits = cache.search_vector_with_benchmark(
                    &db,
                    &config,
                    &model_key,
                    &query_vector,
                    limit,
                    benchmark,
                )?;
                println!("run {run}: {} hits", hits.len());
                Ok(())
            },
        )?;
    }

    Ok(())
}

fn print_help() {
    println!(
        "Usage: cargo run --example benchmark_synthetic_vectors -- --config /path/to/.obsidian-kb.toml --runs 3 --limit 80"
    );
}
