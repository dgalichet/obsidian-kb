use anyhow::{Result, bail};
use std::collections::{BTreeSet, VecDeque};

use crate::benchmark::{self, BenchmarkRun};
use crate::config::AppConfig;
use crate::db::Db;
use crate::error::KbError;
use crate::models::{PropertyFilter, PropertyOperator, SearchFilters, SearchHit, SearchMode};
use crate::paths::KbPaths;
use crate::scoring::{FusedCandidate, reciprocal_rank_fusion};
use crate::{properties, tantivy_index, vector_search};

/// User-facing search options shared by CLI and library calls.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub mode: SearchMode,
    pub limit: usize,
    pub graph: bool,
    pub include_text: bool,
    pub max_chars: usize,
    pub filters: SearchFilters,
}

pub fn search(
    config: &AppConfig,
    query: &str,
    mode: SearchMode,
    limit: usize,
    graph: bool,
    include_text: bool,
    max_chars: usize,
) -> Result<Vec<SearchHit>> {
    search_with_benchmark(
        config,
        query,
        SearchOptions {
            mode,
            limit,
            graph,
            include_text,
            max_chars,
            filters: SearchFilters::default(),
        },
        None,
    )
}

pub fn parse_property_filter(value: &str) -> Result<PropertyFilter> {
    let (key, operator, property_value) = split_property_filter(value)?;
    let key = properties::normalize_key(key);
    let property_value = property_value.trim();
    if key.is_empty() || property_value.is_empty() {
        bail!("property filters must include a non-empty key and value");
    }
    Ok(PropertyFilter {
        key,
        operator,
        value: property_value.to_string(),
    })
}

fn split_property_filter(value: &str) -> Result<(&str, PropertyOperator, &str)> {
    for (token, operator) in [
        (">=", PropertyOperator::Gte),
        ("<=", PropertyOperator::Lte),
        ("!=", PropertyOperator::NotEq),
        (">", PropertyOperator::Gt),
        ("<", PropertyOperator::Lt),
        ("=", PropertyOperator::Eq),
    ] {
        if let Some((key, property_value)) = value.split_once(token) {
            return Ok((key, operator, property_value));
        }
    }
    bail!(
        "property filters must use KEY=VALUE, KEY!=VALUE, KEY>=VALUE, KEY<=VALUE, KEY>VALUE, or KEY<VALUE syntax"
    );
}

/// Executes a search and optionally records phase timings into a benchmark run.
pub fn search_with_benchmark(
    config: &AppConfig,
    query: &str,
    options: SearchOptions,
    benchmark: Option<&mut BenchmarkRun>,
) -> Result<Vec<SearchHit>> {
    search_with_vector_cache(config, query, options, benchmark, None)
}

/// Executes a search with an optional benchmark run and warm vector cache.
pub fn search_with_vector_cache(
    config: &AppConfig,
    query: &str,
    options: SearchOptions,
    mut benchmark: Option<&mut BenchmarkRun>,
    vector_cache: Option<&mut vector_search::VectorSearchCache>,
) -> Result<Vec<SearchHit>> {
    let query = query.trim();
    if query.is_empty() && options.filters.is_empty() {
        return Err(KbError::EmptyQuery.into());
    }
    let paths = KbPaths::from_config(config);
    if !paths.tantivy_dir.exists() {
        return Err(KbError::MissingIndex.into());
    }
    let db = benchmark::time_phase(&mut benchmark, "open_db_ms", || Db::open(&paths.db_path))?;
    let matching_file_ids = benchmark::time_phase(&mut benchmark, "filter_ms", || {
        db.matching_file_ids(&options.filters)
    })?;
    let lexical =
        if !query.is_empty() && matches!(options.mode, SearchMode::Bm25 | SearchMode::Hybrid) {
            benchmark::time_phase(&mut benchmark, "bm25_ms", || {
                tantivy_index::search(
                    &paths.tantivy_dir,
                    query,
                    config.search.bm25_candidates,
                    config.index.remove_diacritics,
                )
            })?
        } else {
            Vec::new()
        };
    let vector = if !query.is_empty()
        && matches!(options.mode, SearchMode::Vector | SearchMode::Hybrid)
        && config.embeddings.enabled
    {
        if let Some(vector_cache) = vector_cache {
            vector_cache.search_with_benchmark(
                &db,
                config,
                query,
                config.search.vector_candidates,
                benchmark.as_deref_mut(),
            )?
        } else {
            vector_search::search_with_benchmark(
                &db,
                config,
                query,
                config.search.vector_candidates,
                benchmark.as_deref_mut(),
            )?
        }
    } else {
        Vec::new()
    };
    if !query.is_empty() && matches!(options.mode, SearchMode::Vector) && vector.is_empty() {
        bail!("no vector results; run `obsidian-kb index` with embeddings enabled");
    }

    let mut fused = if query.is_empty() {
        let file_ids = matching_file_ids.as_ref().cloned().unwrap_or_default();
        benchmark::time_phase(&mut benchmark, "filter_candidates_ms", || {
            filter_only_candidates(&db, &file_ids)
        })?
    } else {
        benchmark::time_phase(&mut benchmark, "fusion_ms", || {
            reciprocal_rank_fusion(
                &lexical,
                &vector,
                config.search.rrf_k,
                config.search.bm25_weight,
                config.search.vector_weight,
            )
        })
    };

    if let Some(file_ids) = matching_file_ids.as_ref() {
        benchmark::time_phase(&mut benchmark, "apply_filters_ms", || {
            filter_candidates(&db, &mut fused, file_ids)
        })?;
    }

    if options.graph {
        benchmark::time_phase(&mut benchmark, "graph_expand_ms", || {
            expand_graph(&db, &mut fused, config, matching_file_ids.as_ref())
        })?;
    }

    fused.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });

    benchmark::time_phase(&mut benchmark, "hydrate_results_ms", || {
        let mut hits = Vec::new();
        let final_limit = if options.limit == 0 {
            config.search.final_top_k
        } else {
            options.limit
        };
        for (index, candidate) in fused.into_iter().take(final_limit).enumerate() {
            if let Some(chunk) = db.load_chunk(&candidate.chunk_id)? {
                hits.push(SearchHit {
                    final_rank: index + 1,
                    final_score: candidate.score,
                    path: chunk.note_path,
                    title: chunk.title,
                    chunk_id: chunk.chunk_id,
                    heading_path: chunk.heading_path.clone(),
                    heading: chunk.heading_path,
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    tags: chunk.tags,
                    snippet: make_snippet(&chunk.text, 240),
                    text: options
                        .include_text
                        .then(|| limit_text(&chunk.text, options.max_chars)),
                    bm25_rank: candidate.lexical_rank,
                    bm25_score: candidate.lexical_score,
                    vector_rank: candidate.vector_rank,
                    vector_score: candidate.vector_score,
                    graph_boost: candidate.graph_boost,
                    score: candidate.score,
                    source: candidate.source,
                    lexical_rank: candidate.lexical_rank,
                    graph: candidate.graph,
                });
            }
        }
        Ok(hits)
    })
}

fn filter_only_candidates(
    db: &Db,
    file_ids: &std::collections::BTreeSet<i64>,
) -> Result<Vec<FusedCandidate>> {
    let chunks = db.chunks_matching_file_ids(file_ids)?;
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| FusedCandidate {
            chunk_id: chunk.chunk_id,
            score: 1.0 / (index + 1) as f32,
            lexical_rank: None,
            lexical_score: None,
            vector_rank: None,
            vector_score: None,
            graph_boost: 0.0,
            source: "filter".to_string(),
            graph: false,
        })
        .collect())
}

fn filter_candidates(
    db: &Db,
    fused: &mut Vec<FusedCandidate>,
    file_ids: &std::collections::BTreeSet<i64>,
) -> Result<()> {
    let mut kept = Vec::with_capacity(fused.len());
    for candidate in fused.drain(..) {
        if let Some(chunk) = db.load_chunk(&candidate.chunk_id)?
            && file_ids.contains(&chunk.file_id)
        {
            kept.push(candidate);
        }
    }
    *fused = kept;
    Ok(())
}

fn expand_graph(
    db: &Db,
    fused: &mut Vec<FusedCandidate>,
    config: &AppConfig,
    allowed_file_ids: Option<&std::collections::BTreeSet<i64>>,
) -> Result<()> {
    let best = fused
        .first()
        .map(|candidate| candidate.score)
        .unwrap_or(0.0);
    if best <= 0.0 {
        return Ok(());
    }
    let mut seen_chunks = fused
        .iter()
        .map(|candidate| candidate.chunk_id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen_notes = BTreeSet::new();
    let seed_top_k = config.search.final_top_k.clamp(1, 5);
    for candidate in fused.iter().take(seed_top_k) {
        if let Some(chunk) = db.load_chunk(&candidate.chunk_id)? {
            seen_notes.insert(chunk.note_path);
        }
    }

    let seeds = fused.iter().take(seed_top_k).cloned().collect::<Vec<_>>();
    let cap = best * 0.35;
    for seed in seeds {
        let Some(seed_chunk) = db.load_chunk(&seed.chunk_id)? else {
            continue;
        };
        let neighbors = graph_neighbors(
            db,
            &seed_chunk.note_path,
            config.search.graph_depth,
            config.search.graph_max_neighbors,
        )?;
        for (neighbor, depth) in neighbors {
            if seen_notes.contains(&neighbor) {
                continue;
            }
            let Some(chunk) = db.first_chunk_for_note(&neighbor)? else {
                continue;
            };
            if let Some(file_ids) = allowed_file_ids
                && !file_ids.contains(&chunk.file_id)
            {
                continue;
            }
            if !seen_chunks.insert(chunk.chunk_id.clone()) {
                continue;
            }
            let score = (seed.score * config.search.graph_weight.powi(depth as i32)).min(cap);
            fused.push(FusedCandidate {
                chunk_id: chunk.chunk_id,
                score,
                lexical_rank: None,
                lexical_score: None,
                vector_rank: None,
                vector_score: None,
                graph_boost: score,
                source: "graph_expanded".to_string(),
                graph: true,
            });
            seen_notes.insert(neighbor);
        }
    }
    Ok(())
}

fn graph_neighbors(
    db: &Db,
    seed_path: &str,
    depth: usize,
    max_neighbors: usize,
) -> Result<Vec<(String, usize)>> {
    if depth == 0 || max_neighbors == 0 {
        return Ok(Vec::new());
    }

    let mut queue = VecDeque::from([(seed_path.to_string(), 0usize)]);
    let mut seen = BTreeSet::from([seed_path.to_string()]);
    let mut neighbors = Vec::new();

    while let Some((path, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }
        let next_depth = current_depth + 1;
        for neighbor in db.neighbor_paths(&path, max_neighbors)? {
            if !seen.insert(neighbor.clone()) {
                continue;
            }
            neighbors.push((neighbor.clone(), next_depth));
            if next_depth < depth {
                queue.push_back((neighbor, next_depth));
            }
        }
    }

    Ok(neighbors)
}

fn make_snippet(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    limit_text(&compact, max_chars)
}

fn limit_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut limited = text.chars().take(max_chars).collect::<String>();
    limited.push_str("...");
    limited
}
