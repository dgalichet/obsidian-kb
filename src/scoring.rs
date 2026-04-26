use std::collections::BTreeMap;

use crate::models::SearchCandidate;

#[derive(Debug, Clone)]
pub struct FusedCandidate {
    pub chunk_id: String,
    pub score: f32,
    pub lexical_rank: Option<usize>,
    pub lexical_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub vector_score: Option<f32>,
    pub graph_boost: f32,
    pub source: String,
    pub graph: bool,
}

pub fn reciprocal_rank_fusion(
    lexical: &[SearchCandidate],
    vector: &[SearchCandidate],
    rrf_k: f32,
    lexical_weight: f32,
    vector_weight: f32,
) -> Vec<FusedCandidate> {
    let mut scores = BTreeMap::<String, FusedCandidate>::new();
    for (index, candidate) in lexical.iter().enumerate() {
        let rank = index + 1;
        let entry = scores
            .entry(candidate.chunk_id.clone())
            .or_insert_with(|| empty_candidate(candidate.chunk_id.clone()));
        entry.lexical_rank = Some(rank);
        entry.lexical_score = Some(candidate.score);
        entry.score += lexical_weight / (rrf_k + rank as f32);
    }
    for (index, candidate) in vector.iter().enumerate() {
        let rank = index + 1;
        let entry = scores
            .entry(candidate.chunk_id.clone())
            .or_insert_with(|| empty_candidate(candidate.chunk_id.clone()));
        entry.vector_rank = Some(rank);
        entry.vector_score = Some(candidate.score);
        entry.score += vector_weight / (rrf_k + rank as f32);
    }

    let mut fused = scores
        .into_values()
        .map(|mut candidate| {
            candidate.source = match (candidate.lexical_rank, candidate.vector_rank) {
                (Some(_), Some(_)) => "hybrid",
                (Some(_), None) => "lexical",
                (None, Some(_)) => "vector",
                (None, None) => "unknown",
            }
            .to_string();
            candidate
        })
        .collect::<Vec<_>>();
    fused.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| source_rank(&left.source).cmp(&source_rank(&right.source)))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    fused
}

fn empty_candidate(chunk_id: String) -> FusedCandidate {
    FusedCandidate {
        chunk_id,
        score: 0.0,
        lexical_rank: None,
        lexical_score: None,
        vector_rank: None,
        vector_score: None,
        graph_boost: 0.0,
        source: String::new(),
        graph: false,
    }
}

fn source_rank(source: &str) -> usize {
    match source {
        "hybrid" => 0,
        "lexical" => 1,
        "vector" => 2,
        _ => 3,
    }
}
