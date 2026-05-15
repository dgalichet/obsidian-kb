use anyhow::{Result, bail};
use std::collections::BTreeSet;

use crate::config::AppConfig;
use crate::db::Db;
use crate::error::KbError;
use crate::models::{SearchHit, SearchMode};
use crate::paths::KbPaths;
use crate::scoring::{FusedCandidate, reciprocal_rank_fusion};
use crate::{tantivy_index, vector_search};

pub fn search(
    config: &AppConfig,
    query: &str,
    mode: SearchMode,
    limit: usize,
    graph: bool,
    include_text: bool,
    max_chars: usize,
) -> Result<Vec<SearchHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(KbError::EmptyQuery.into());
    }
    let paths = KbPaths::from_config(config);
    if !paths.tantivy_dir.exists() {
        return Err(KbError::MissingIndex.into());
    }
    let db = Db::open(&paths.db_path)?;
    let lexical = if matches!(mode, SearchMode::Bm25 | SearchMode::Hybrid) {
        tantivy_index::search(
            &paths.tantivy_dir,
            query,
            config.search.bm25_candidates,
            config.index.remove_diacritics,
        )?
    } else {
        Vec::new()
    };
    let vector =
        if matches!(mode, SearchMode::Vector | SearchMode::Hybrid) && config.embeddings.enabled {
            vector_search::search(&db, config, query, config.search.vector_candidates)?
        } else {
            Vec::new()
        };
    if matches!(mode, SearchMode::Vector) && vector.is_empty() {
        bail!("no vector results; run `obsidian-kb index` with embeddings enabled");
    }

    let mut fused = reciprocal_rank_fusion(
        &lexical,
        &vector,
        config.search.rrf_k,
        config.search.bm25_weight,
        config.search.vector_weight,
    );

    if graph {
        expand_graph(&db, &mut fused, config)?;
    }

    fused.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });

    let mut hits = Vec::new();
    let final_limit = if limit == 0 {
        config.search.final_top_k
    } else {
        limit
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
                text: include_text.then(|| limit_text(&chunk.text, max_chars)),
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
}

fn expand_graph(db: &Db, fused: &mut Vec<FusedCandidate>, config: &AppConfig) -> Result<()> {
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
        let neighbors =
            db.neighbor_paths(&seed_chunk.note_path, config.search.graph_max_neighbors)?;
        for neighbor in neighbors {
            if seen_notes.contains(&neighbor) {
                continue;
            }
            let Some(chunk) = db.first_chunk_for_note(&neighbor)? else {
                continue;
            };
            if !seen_chunks.insert(chunk.chunk_id.clone()) {
                continue;
            }
            let score = (seed.score * config.search.graph_weight).min(cap);
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
