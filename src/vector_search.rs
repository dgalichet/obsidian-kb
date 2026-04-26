use anyhow::Result;
use rayon::prelude::*;

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
    let mut embedder = FastEmbedder::new(config)?;
    let model_key = embedder.model_key();
    let query = normalize_text(query, config.index.remove_diacritics);
    let query_vector = embedder.embed_query(&query)?;
    let embeddings = db.load_embeddings(&model_key)?;
    Ok(score_embeddings(
        &query_vector,
        embeddings,
        config.embedding_min_cosine(),
        limit,
    ))
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
