use anyhow::{Context, Result, bail};
use obsidian_kb::{config, db::Db, embeddings::encode_vector, paths::KbPaths};
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
        let vector = deterministic_vector(&chunk.chunk_id, dim);
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

fn deterministic_vector(seed: &str, dim: usize) -> Vec<f32> {
    let hash = blake3::hash(seed.as_bytes());
    let mut state = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().unwrap());
    let mut vector = Vec::with_capacity(dim);
    let mut norm = 0.0f32;

    for _ in 0..dim {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let unit = ((state >> 40) as f32) / ((1u32 << 24) as f32);
        let value = unit.mul_add(2.0, -1.0);
        norm += value * value;
        vector.push(value);
    }

    let norm = norm.sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}
