mod common;

use assert_cmd::Command;
use obsidian_kb::embeddings::encode_vector;
use obsidian_kb::{config, db::Db};
use serde_json::Value;

#[test]
fn related_returns_similar_notes_for_indexed_note() {
    let (_temp, vault) = common::temp_vault();
    index_without_embeddings(&vault);
    seed_embeddings(&vault);

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "related",
            "--vault",
            vault.to_str().unwrap(),
            "ai/contexte.md",
            "--top",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["source"]["kind"], "note");
    assert_eq!(value["source"]["path"], "ai/contexte.md");
    assert_eq!(value["notes"][0]["path"], "ai/rag.md");
    assert_ne!(value["notes"][0]["path"], value["source"]["path"]);
    assert!(value["notes"][0]["score"].as_f64().unwrap() > 0.9);
}

#[test]
fn related_writes_benchmark_phases() {
    let (_temp, vault) = common::temp_vault();
    index_without_embeddings(&vault);
    seed_embeddings(&vault);

    let mut app_config = config::load_existing(Some(&vault), None).unwrap();
    app_config.benchmark.enabled = true;
    config::save_config(&app_config).unwrap();

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "related",
            "--vault",
            vault.to_str().unwrap(),
            "ai/contexte.md",
            "--top",
            "2",
            "--json",
        ])
        .assert()
        .success();

    let log_path = vault.join(".obsidian-kb/benchmarks.jsonl");
    let content = std::fs::read_to_string(log_path).unwrap();
    let record: Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();

    assert_eq!(record["command"], "related");
    assert_eq!(record["status"], "ok");
    assert_eq!(record["vector_backend"], "brute_force");
    assert_eq!(record["source_kind"], "note");
    assert!(record["related_result_count"].as_u64().unwrap() >= 1);
    assert!(record["phases"]["related_source_ms"].as_f64().is_some());
    assert!(record["phases"]["vector_ms"].as_f64().is_some());
    assert!(
        record["phases"]["vector_load_embeddings_ms"]
            .as_f64()
            .is_some()
    );
    assert!(
        record["phases"]["vector_score_embeddings_ms"]
            .as_f64()
            .is_some()
    );
    assert!(
        record["phases"]["related_aggregate_notes_ms"]
            .as_f64()
            .is_some()
    );
}

fn index_without_embeddings(vault: &std::path::Path) {
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
}

fn seed_embeddings(vault: &std::path::Path) {
    let db_path = vault.join(".obsidian-kb/metadata.sqlite");
    let db = Db::open(&db_path).unwrap();
    let chunks = db.load_all_chunks().unwrap();
    for chunk in chunks {
        let vector = if chunk.note_path == "ai/contexte.md" {
            [1.0, 0.0, 0.0]
        } else if chunk.note_path == "ai/rag.md" {
            [0.97, 0.03, 0.0]
        } else if chunk.note_path == "ai/lost-in-the-middle.md" {
            [0.65, 0.35, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        db.insert_embedding(
            &chunk.chunk_id,
            "fastembed:MultilingualE5Small",
            3,
            &encode_vector(&vector),
            &chunk.text_hash,
        )
        .unwrap();
    }
}
