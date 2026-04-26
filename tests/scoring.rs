use obsidian_kb::{models::SearchCandidate, scoring};

#[test]
fn rrf_rewards_candidates_seen_by_both_rankers() {
    let lexical = vec![
        SearchCandidate {
            chunk_id: "a".to_string(),
            score: 9.0,
        },
        SearchCandidate {
            chunk_id: "b".to_string(),
            score: 8.0,
        },
    ];
    let vector = vec![
        SearchCandidate {
            chunk_id: "b".to_string(),
            score: 0.9,
        },
        SearchCandidate {
            chunk_id: "c".to_string(),
            score: 0.8,
        },
    ];
    let fused = scoring::reciprocal_rank_fusion(&lexical, &vector, 60.0, 1.0, 1.0);
    assert_eq!(fused[0].chunk_id, "b");
    assert_eq!(fused[0].source, "hybrid");
    assert_eq!(fused[0].lexical_rank, Some(2));
    assert_eq!(fused[0].lexical_score, Some(8.0));
    assert_eq!(fused[0].vector_rank, Some(1));
    assert_eq!(fused[0].vector_score, Some(0.9));
}
