mod common;

use assert_cmd::Command;
use obsidian_kb::embeddings::encode_vector;
use obsidian_kb::{db::Db, vector_search};

#[test]
fn unchanged_chunk_embeddings_are_preserved_across_changed_only_indexing() {
    let (_temp, vault) = common::temp_vault();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "index",
            "--vault",
            vault.to_str().unwrap(),
            "--no-embeddings",
        ])
        .assert()
        .success();

    let db_path = vault.join(".obsidian-kb/metadata.sqlite");
    let db = Db::open(&db_path).unwrap();
    let chunk = db.load_all_chunks().unwrap().remove(0);
    let vector = encode_vector(&[1.0, 0.0, 0.0]);
    db.insert_embedding(
        &chunk.chunk_id,
        "fastembed:MultilingualE5Small",
        3,
        &vector,
        &chunk.text_hash,
    )
    .unwrap();
    drop(db);

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "index",
            "--vault",
            vault.to_str().unwrap(),
            "--no-embeddings",
        ])
        .assert()
        .success();

    let db = Db::open(&db_path).unwrap();
    let embeddings = db.load_embeddings("fastembed:MultilingualE5Small").unwrap();
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].0, chunk.chunk_id);
}

#[test]
fn vector_scoring_returns_top_cosine_candidates() {
    let embeddings = vec![
        ("near".to_string(), 3, encode_vector(&[0.9, 0.1, 0.0])),
        ("far".to_string(), 3, encode_vector(&[0.0, 1.0, 0.0])),
    ];
    let hits = vector_search::score_embeddings(&[1.0, 0.0, 0.0], embeddings, 0.0, 2);

    assert_eq!(hits[0].chunk_id, "near");
    assert!(hits[0].score > hits[1].score);
}
