use anyhow::{Context, Result, bail};
use obsidian_kb::{
    config, db::Db, embeddings::encode_vector, paths::KbPaths, synthetic::deterministic_unit_vector,
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args.next().context("missing value after --config")?;
                config_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => bail!("unknown argument `{other}`"),
        }
    }

    let config =
        config::load_existing(None, config_path.as_deref()).context("failed to load config")?;
    let paths = KbPaths::from_config(&config);
    let db = Db::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    let chunks = db.load_all_chunks()?;
    let model_key = config.embedding_model_key();
    let dim = config.embedding_dimensions();

    for chunk in &chunks {
        let vector = deterministic_unit_vector(&chunk.chunk_id, dim);
        let encoded = encode_vector(&vector);
        db.insert_embedding(&chunk.chunk_id, &model_key, dim, &encoded, &chunk.text_hash)?;
    }

    println!(
        "seeded {} synthetic {}-dim embeddings for {}",
        chunks.len(),
        dim,
        model_key
    );
    Ok(())
}

fn print_help() {
    println!(
        "Usage: cargo run --example seed_synthetic_embeddings -- --config /path/to/.obsidian-kb.toml"
    );
}
