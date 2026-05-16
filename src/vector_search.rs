use anyhow::Result;
use rayon::prelude::*;
use std::time::Instant;

use crate::benchmark::{self, BenchmarkRun};
use crate::config::AppConfig;
use crate::db::Db;
use crate::embeddings::{FastEmbedder, cosine, decode_vector, encode_vector};
use crate::models::SearchCandidate;
use crate::normalization::normalize_text;

pub fn rebuild_embeddings(db: &Db, config: &AppConfig) -> Result<usize> {
    let chunks = db.load_all_chunks()?;
    let model_key = config.embedding_model_key();
    let existing = db.load_embedding_hashes(&model_key)?;
    let pending = chunks
        .iter()
        .filter(|chunk| existing.get(&chunk.chunk_id) != Some(&chunk.text_hash))
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(0);
    }

    let mut embedder = FastEmbedder::new(config)?;
    let model_key = embedder.model_key();

    for batch in pending.chunks(config.embeddings.batch_size.max(1)) {
        let texts = batch
            .iter()
            .map(|chunk| {
                normalize_text(
                    &format!(
                        "{}\n{}\n{}\n{}",
                        chunk.title,
                        chunk.heading_path,
                        chunk.tags.join(" "),
                        chunk.text
                    ),
                    config.index.remove_diacritics,
                )
            })
            .collect::<Vec<_>>();
        let vectors = embedder.embed_passages(&texts)?;
        for (chunk, vector) in batch.iter().zip(vectors) {
            let encoded = encode_vector(&vector);
            db.insert_embedding(
                &chunk.chunk_id,
                &model_key,
                vector.len(),
                &encoded,
                &chunk.text_hash,
            )?;
        }
    }

    Ok(pending.len())
}

pub fn search(
    db: &Db,
    config: &AppConfig,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchCandidate>> {
    search_with_benchmark(db, config, query, limit, None)
}

/// Runs vector search and optionally records detailed vector phase timings.
pub fn search_with_benchmark(
    db: &Db,
    config: &AppConfig,
    query: &str,
    limit: usize,
    mut benchmark: Option<&mut BenchmarkRun>,
) -> Result<Vec<SearchCandidate>> {
    let started = Instant::now();
    let result = (|| {
        let mut embedder =
            benchmark::time_phase(&mut benchmark, "vector_embedder_init_ms", || {
                FastEmbedder::new(config)
            })?;
        let model_key = embedder.model_key();
        let query = benchmark::time_phase(&mut benchmark, "vector_normalize_query_ms", || {
            normalize_text(query, config.index.remove_diacritics)
        });
        let query_vector = benchmark::time_phase(&mut benchmark, "vector_embed_query_ms", || {
            embedder.embed_query(&query)
        })?;
        let embeddings =
            benchmark::time_phase(&mut benchmark, "vector_load_embeddings_ms", || {
                db.load_embeddings(&model_key)
            })?;
        if let Some(benchmark) = benchmark.as_deref_mut() {
            benchmark.set_field("vector_dimensions", query_vector.len());
            benchmark.set_field("vector_embedding_count", embeddings.len());
        }
        Ok(benchmark::time_phase(
            &mut benchmark,
            "vector_score_embeddings_ms",
            || {
                score_embeddings(
                    &query_vector,
                    embeddings,
                    config.embedding_min_cosine(),
                    limit,
                )
            },
        ))
    })();
    if let Some(benchmark) = benchmark {
        benchmark.record_phase("vector_ms", started.elapsed());
    }
    result
}

pub fn score_embeddings(
    query_vector: &[f32],
    embeddings: Vec<(String, usize, Vec<u8>)>,
    min_cosine: f32,
    limit: usize,
) -> Vec<SearchCandidate> {
    let mut candidates = embeddings
        .into_par_iter()
        .filter_map(|(chunk_id, dim, bytes)| {
            if dim != query_vector.len() {
                return None;
            }
            let vector = decode_vector(&bytes);
            let score = cosine(query_vector, &vector);
            (score >= min_cosine).then_some(SearchCandidate { chunk_id, score })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    candidates.truncate(limit);
    candidates
}
