use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::time::Instant;

use crate::benchmark::{self, BenchmarkRun};
use crate::config::AppConfig;
use crate::db::Db;
use crate::embeddings::{FastEmbedder, decode_vector};
use crate::error::KbError;
use crate::models::{RelatedChunk, RelatedNote, RelatedReport, RelatedSource, SearchCandidate};
use crate::normalization::normalize_text;
use crate::paths::KbPaths;
use crate::vector_search;

#[derive(Debug, Clone)]
pub struct RelatedOptions {
    pub limit: usize,
    pub candidates: usize,
}

#[derive(Debug, Clone)]
pub enum RelatedInput {
    Note(String),
    Text(String),
}

/// Finds indexed notes semantically related to an indexed note or draft text.
pub fn find_related_with_benchmark(
    config: &AppConfig,
    input: RelatedInput,
    options: RelatedOptions,
    benchmark: Option<&mut BenchmarkRun>,
) -> Result<RelatedReport> {
    find_related_with_vector_cache(config, input, options, benchmark, None)
}

/// Finds related notes while optionally reusing a warm vector search cache.
pub fn find_related_with_vector_cache(
    config: &AppConfig,
    input: RelatedInput,
    options: RelatedOptions,
    mut benchmark: Option<&mut BenchmarkRun>,
    vector_cache: Option<&mut vector_search::VectorSearchCache>,
) -> Result<RelatedReport> {
    if !config.embeddings.enabled {
        bail!("embeddings are disabled; enable embeddings and run `obsidian-kb index`");
    }

    let paths = KbPaths::from_config(config);
    if !paths.db_path.exists() {
        return Err(KbError::MissingIndex.into());
    }

    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || Db::open(&paths.db_path))?;
    let model_key = config.embedding_model_key();
    let source_started = Instant::now();
    let source_result = source_vector(&db, config, &model_key, input, benchmark.as_deref_mut());
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.record_phase("related_source_ms", source_started.elapsed());
    }
    let (source, source_vector, source_path) = source_result?;
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.set_field("related_source_dimensions", source_vector.len());
    }

    let final_limit = final_limit(options.limit, config);
    let candidate_limit = candidate_limit(options.candidates, final_limit, config);
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.set_field("related_effective_top", final_limit);
        benchmark.set_field("related_effective_candidates", candidate_limit);
        benchmark.set_field("vector_backend", "brute_force");
    }

    let vector_started = Instant::now();
    let candidates = if let Some(vector_cache) = vector_cache {
        vector_cache.search_vector_with_benchmark(
            &db,
            config,
            &model_key,
            &source_vector,
            candidate_limit,
            benchmark.as_deref_mut(),
        )
    } else {
        vector_search::search_vector_with_benchmark(
            &db,
            config,
            &model_key,
            &source_vector,
            candidate_limit,
            benchmark.as_deref_mut(),
        )
    };
    if let Some(benchmark) = benchmark.as_deref_mut() {
        benchmark.record_phase("vector_ms", vector_started.elapsed());
    }
    let candidates = candidates?;
    let notes = benchmark::time_phase(&mut benchmark, "related_aggregate_notes_ms", || {
        aggregate_notes(&db, candidates, source_path.as_deref(), final_limit)
    })?;

    if let Some(benchmark) = benchmark {
        benchmark.set_field("related_result_count", notes.len());
    }

    Ok(RelatedReport { source, notes })
}

fn source_vector(
    db: &Db,
    config: &AppConfig,
    model_key: &str,
    input: RelatedInput,
    mut benchmark: Option<&mut BenchmarkRun>,
) -> Result<(RelatedSource, Vec<f32>, Option<String>)> {
    match input {
        RelatedInput::Note(identifier) => {
            let path = db
                .resolve_note_path(&identifier)?
                .with_context(|| format!("note not found: {identifier}"))?;
            let summary = db
                .note_summary(&path)?
                .with_context(|| format!("note not found: {path}"))?;
            let embeddings =
                benchmark::time_phase(&mut benchmark, "related_load_source_embeddings_ms", || {
                    db.load_note_embeddings(&path, model_key)
                })?;
            let vector = average_embeddings(&embeddings).with_context(|| {
                format!("no embeddings for note `{path}`; run `obsidian-kb index`")
            })?;
            if let Some(benchmark) = benchmark {
                benchmark.set_field("related_source_kind", "note");
                benchmark.set_field("related_source_chunks", embeddings.len());
            }
            Ok((
                RelatedSource::Note {
                    identifier,
                    path: summary.path.clone(),
                    title: summary.title,
                    source_chunks: embeddings.len(),
                },
                vector,
                Some(summary.path),
            ))
        }
        RelatedInput::Text(text) => {
            let normalized = normalize_text(&text, config.index.remove_diacritics);
            if normalized.trim().is_empty() {
                bail!("related text is empty");
            }
            let mut embedder =
                benchmark::time_phase(&mut benchmark, "related_embedder_init_ms", || {
                    FastEmbedder::new(config)
                })?;
            let vectors = benchmark::time_phase(&mut benchmark, "related_embed_text_ms", || {
                embedder.embed_passages(&[normalized])
            })?;
            let vector = vectors
                .into_iter()
                .next()
                .context("fastembed returned no text embedding")?;
            if let Some(benchmark) = benchmark {
                benchmark.set_field("related_source_kind", "text");
                benchmark.set_field("related_source_chars", text.chars().count());
            }
            Ok((
                RelatedSource::Text {
                    chars: text.chars().count(),
                },
                vector,
                None,
            ))
        }
    }
}

fn aggregate_notes(
    db: &Db,
    candidates: Vec<SearchCandidate>,
    source_path: Option<&str>,
    limit: usize,
) -> Result<Vec<RelatedNote>> {
    let mut notes = BTreeMap::<String, NoteAccumulator>::new();
    for candidate in candidates {
        let Some(chunk) = db.load_chunk(&candidate.chunk_id)? else {
            continue;
        };
        if Some(chunk.note_path.as_str()) == source_path {
            continue;
        }
        let entry = notes
            .entry(chunk.note_path.clone())
            .or_insert_with(|| NoteAccumulator {
                path: chunk.note_path.clone(),
                document_kind: chunk.document_kind,
                title: chunk.title.clone(),
                tags: chunk.tags.clone(),
                chunks: Vec::new(),
            });
        entry.chunks.push(RelatedChunk {
            chunk_id: chunk.chunk_id,
            score: candidate.score,
            document_kind: chunk.document_kind,
            heading_path: chunk.heading_path,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            start_page: chunk.start_page,
            end_page: chunk.end_page,
            snippet: make_snippet(&chunk.text, 180),
        });
    }

    let mut related = notes
        .into_values()
        .filter_map(|mut note| {
            note.chunks.sort_by(|left, right| {
                right
                    .score
                    .total_cmp(&left.score)
                    .then_with(|| left.chunk_id.cmp(&right.chunk_id))
            });
            let best = note.chunks.first()?;
            Some(RelatedNote {
                rank: 0,
                score: best.score,
                path: note.path,
                document_kind: note.document_kind,
                title: note.title,
                tags: note.tags,
                best_chunk_id: best.chunk_id.clone(),
                best_heading: best.heading_path.clone(),
                best_score: best.score,
                matched_chunks: note.chunks.len(),
                chunks: note.chunks,
            })
        })
        .collect::<Vec<_>>();
    related.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    related.truncate(limit);
    for (index, note) in related.iter_mut().enumerate() {
        note.rank = index + 1;
    }
    Ok(related)
}

fn average_embeddings(embeddings: &[(String, usize, Vec<u8>)]) -> Option<Vec<f32>> {
    let mut sum = Vec::<f32>::new();
    let mut count = 0usize;
    for (_, dim, bytes) in embeddings {
        let vector = decode_vector(bytes);
        if *dim != vector.len() || vector.is_empty() {
            continue;
        }
        if sum.is_empty() {
            sum.resize(vector.len(), 0.0);
        }
        if sum.len() != vector.len() {
            continue;
        }
        for (slot, value) in sum.iter_mut().zip(vector) {
            *slot += value;
        }
        count += 1;
    }
    if count == 0 {
        return None;
    }
    for value in &mut sum {
        *value /= count as f32;
    }
    Some(sum)
}

fn final_limit(limit: usize, config: &AppConfig) -> usize {
    if limit == 0 {
        config.search.final_top_k.max(1)
    } else {
        limit
    }
}

fn candidate_limit(candidates: usize, limit: usize, config: &AppConfig) -> usize {
    if candidates == 0 {
        config
            .search
            .vector_candidates
            .max(limit.saturating_mul(5))
            .max(1)
    } else {
        candidates.max(limit)
    }
}

fn make_snippet(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if max_chars == 0 || compact.chars().count() <= max_chars {
        return compact;
    }
    let mut limited = compact.chars().take(max_chars).collect::<String>();
    limited.push_str("...");
    limited
}

struct NoteAccumulator {
    path: String,
    document_kind: crate::models::DocumentKind,
    title: String,
    tags: Vec<String>,
    chunks: Vec<RelatedChunk>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::encode_vector;

    #[test]
    fn averages_source_embeddings() {
        let embeddings = vec![
            ("a".to_string(), 3, encode_vector(&[1.0, 0.0, 0.0])),
            ("b".to_string(), 3, encode_vector(&[0.0, 1.0, 0.0])),
        ];
        let average = average_embeddings(&embeddings).unwrap();
        assert_eq!(average, vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn candidate_limit_defaults_above_top() {
        let config = AppConfig::default_for_vault_in(
            std::path::Path::new("tests/fixtures/sample_vault"),
            None,
            std::path::Path::new("."),
        )
        .unwrap();
        assert!(candidate_limit(0, 20, &config) >= 100);
    }
}
