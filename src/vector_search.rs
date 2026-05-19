use anyhow::Result;
use rayon::prelude::*;
use std::time::{Duration, Instant};

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

/// Keeps the local embedding model warm across multiple vector searches.
#[derive(Default)]
pub struct VectorSearchCache {
    embedder: Option<FastEmbedder>,
    model_key: Option<String>,
    embeddings: Option<EmbeddingCache>,
    last_used: Option<Instant>,
}

struct EmbeddingCache {
    model_key: String,
    embeddings: Vec<CachedEmbedding>,
}

struct CachedEmbedding {
    chunk_id: String,
    vector: Vec<f32>,
}

impl VectorSearchCache {
    /// Initializes the embedding model before the first search.
    pub fn warm_up(&mut self, config: &AppConfig) -> Result<()> {
        if self.embedder.is_none() {
            let embedder = FastEmbedder::new(config)?;
            self.model_key = Some(embedder.model_key());
            self.embedder = Some(embedder);
        }
        self.last_used = Some(Instant::now());
        Ok(())
    }

    /// Drops the cached embedding model and stored embeddings, releasing memory.
    pub fn unload(&mut self) -> bool {
        let was_loaded = self.embedder.is_some() || self.embeddings.is_some();
        self.embedder = None;
        self.model_key = None;
        self.embeddings = None;
        self.last_used = None;
        was_loaded
    }

    /// Drops the cached model when it has not been used within `timeout`.
    pub fn unload_if_idle(&mut self, timeout: Duration) -> bool {
        if timeout.is_zero() {
            return false;
        }
        let Some(last_used) = self.last_used else {
            return false;
        };
        if last_used.elapsed() >= timeout {
            self.unload()
        } else {
            false
        }
    }

    /// Returns whether the embedding model is currently loaded in memory.
    pub fn is_loaded(&self) -> bool {
        self.embedder.is_some()
    }

    /// Returns whether stored embeddings are currently cached in memory.
    pub fn embeddings_loaded(&self) -> bool {
        self.embeddings.is_some()
    }

    /// Returns the number of stored embeddings currently cached in memory.
    pub fn cached_embedding_count(&self) -> usize {
        self.embeddings
            .as_ref()
            .map(|cache| cache.embeddings.len())
            .unwrap_or_default()
    }

    /// Loads stored embeddings for the configured model into memory.
    pub fn warm_up_embeddings(&mut self, db: &Db, config: &AppConfig) -> Result<()> {
        let model_key = self
            .model_key
            .clone()
            .unwrap_or_else(|| config.embedding_model_key());
        self.cached_embeddings_with_benchmark(db, &model_key, None)?;
        self.last_used = Some(Instant::now());
        Ok(())
    }

    /// Runs vector search using the cached model when available.
    pub fn search_with_benchmark(
        &mut self,
        db: &Db,
        config: &AppConfig,
        query: &str,
        limit: usize,
        mut benchmark: Option<&mut BenchmarkRun>,
    ) -> Result<Vec<SearchCandidate>> {
        let started = Instant::now();
        let result = (|| {
            if self.embedder.is_none() {
                let embedder =
                    benchmark::time_phase(&mut benchmark, "vector_embedder_init_ms", || {
                        FastEmbedder::new(config)
                    })?;
                self.model_key = Some(embedder.model_key());
                self.embedder = Some(embedder);
            } else if let Some(benchmark) = benchmark.as_deref_mut() {
                benchmark.record_phase("vector_embedder_init_ms", Duration::ZERO);
                benchmark.set_field("vector_embedder_cached", true);
            }

            let model_key = self
                .model_key
                .clone()
                .unwrap_or_else(|| config.embedding_model_key());
            let query = benchmark::time_phase(&mut benchmark, "vector_normalize_query_ms", || {
                normalize_text(query, config.index.remove_diacritics)
            });
            let query_vector = {
                let embedder = self
                    .embedder
                    .as_mut()
                    .expect("vector embedder is initialized");
                benchmark::time_phase(&mut benchmark, "vector_embed_query_ms", || {
                    embedder.embed_query(&query)
                })?
            };
            let candidates = self.search_vector_with_benchmark(
                db,
                config,
                &model_key,
                &query_vector,
                limit,
                benchmark.as_deref_mut(),
            )?;
            self.last_used = Some(Instant::now());
            Ok(candidates)
        })();
        if let Some(benchmark) = benchmark {
            benchmark.record_phase("vector_ms", started.elapsed());
        }
        result
    }

    /// Scores a prepared query vector using cached stored embeddings when available.
    pub fn search_vector_with_benchmark(
        &mut self,
        db: &Db,
        config: &AppConfig,
        model_key: &str,
        query_vector: &[f32],
        limit: usize,
        mut benchmark: Option<&mut BenchmarkRun>,
    ) -> Result<Vec<SearchCandidate>> {
        let embeddings =
            self.cached_embeddings_with_benchmark(db, model_key, benchmark.as_deref_mut())?;
        if let Some(benchmark) = benchmark.as_deref_mut() {
            benchmark.set_field("vector_dimensions", query_vector.len());
            benchmark.set_field("vector_embedding_count", embeddings.len());
        }
        let candidates =
            benchmark::time_phase(&mut benchmark, "vector_score_embeddings_ms", || {
                score_cached_embeddings(
                    query_vector,
                    embeddings,
                    config.embedding_min_cosine(),
                    limit,
                )
            });
        self.last_used = Some(Instant::now());
        Ok(candidates)
    }

    fn cached_embeddings_with_benchmark(
        &mut self,
        db: &Db,
        model_key: &str,
        mut benchmark: Option<&mut BenchmarkRun>,
    ) -> Result<&[CachedEmbedding]> {
        let cache_hit = self
            .embeddings
            .as_ref()
            .map(|cache| cache.model_key == model_key)
            .unwrap_or(false);
        if cache_hit {
            if let Some(benchmark) = benchmark.as_deref_mut() {
                benchmark.record_phase("vector_load_embeddings_ms", Duration::ZERO);
                benchmark.record_phase("vector_decode_embeddings_ms", Duration::ZERO);
                benchmark.set_field("vector_embeddings_cached", true);
            }
        } else {
            let rows = benchmark::time_phase(&mut benchmark, "vector_load_embeddings_ms", || {
                db.load_embeddings(model_key)
            })?;
            let embeddings =
                benchmark::time_phase(&mut benchmark, "vector_decode_embeddings_ms", || {
                    decode_cached_embeddings(rows)
                });
            self.embeddings = Some(EmbeddingCache {
                model_key: model_key.to_string(),
                embeddings,
            });
            if let Some(benchmark) = benchmark {
                benchmark.set_field("vector_embeddings_cached", false);
            }
        }

        Ok(&self
            .embeddings
            .as_ref()
            .expect("embedding cache is initialized")
            .embeddings)
    }
}

/// Runs vector search and optionally records detailed vector phase timings.
pub fn search_with_benchmark(
    db: &Db,
    config: &AppConfig,
    query: &str,
    limit: usize,
    benchmark: Option<&mut BenchmarkRun>,
) -> Result<Vec<SearchCandidate>> {
    VectorSearchCache::default().search_with_benchmark(db, config, query, limit, benchmark)
}

/// Scores a prepared query vector against all stored embeddings for `model_key`.
pub fn search_vector_with_benchmark(
    db: &Db,
    config: &AppConfig,
    model_key: &str,
    query_vector: &[f32],
    limit: usize,
    mut benchmark: Option<&mut BenchmarkRun>,
) -> Result<Vec<SearchCandidate>> {
    let embeddings = benchmark::time_phase(&mut benchmark, "vector_load_embeddings_ms", || {
        db.load_embeddings(model_key)
    })?;
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.set_field("vector_dimensions", query_vector.len());
        benchmark.set_field("vector_embedding_count", embeddings.len());
        benchmark.set_field("vector_embeddings_cached", false);
    }
    Ok(benchmark::time_phase(
        &mut benchmark,
        "vector_score_embeddings_ms",
        || {
            score_embeddings(
                query_vector,
                embeddings,
                config.embedding_min_cosine(),
                limit,
            )
        },
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

fn decode_cached_embeddings(rows: Vec<(String, usize, Vec<u8>)>) -> Vec<CachedEmbedding> {
    rows.into_iter()
        .filter_map(|(chunk_id, dim, bytes)| {
            let vector = decode_vector(&bytes);
            (dim == vector.len() && !vector.is_empty())
                .then_some(CachedEmbedding { chunk_id, vector })
        })
        .collect()
}

fn score_cached_embeddings(
    query_vector: &[f32],
    embeddings: &[CachedEmbedding],
    min_cosine: f32,
    limit: usize,
) -> Vec<SearchCandidate> {
    let mut candidates = embeddings
        .par_iter()
        .filter_map(|embedding| {
            if embedding.vector.len() != query_vector.len() {
                return None;
            }
            let score = cosine(query_vector, &embedding.vector);
            (score >= min_cosine).then_some(SearchCandidate {
                chunk_id: embedding.chunk_id.clone(),
                score,
            })
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
