mod common;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn bm25_search_finds_exact_note() {
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
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--top",
            "10",
            "mesh namespace routing",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("aws/service-connect.md"));
}

#[test]
fn graph_search_adds_direct_neighbors() {
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
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--expand-graph",
            "--top",
            "5",
            "mesh namespace routing",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("aws/app-mesh.md"));
}

#[test]
fn search_json_includes_explainability_fields_and_sanitizes_queries() {
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

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--json",
            "Service Connect TLS App Mesh (",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let hits: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(hits[0]["final_rank"].as_u64().unwrap() >= 1);
    assert!(hits[0]["final_score"].as_f64().unwrap() > 0.0);
    assert!(hits[0]["bm25_rank"].as_u64().unwrap() >= 1);
    assert!(hits[0]["bm25_score"].as_f64().unwrap() > 0.0);
    assert!(hits[0]["heading_path"].is_string());
    assert!(hits[0]["chunk_id"].is_string());
    assert!(hits[0].get("text").is_none());
}

#[test]
fn search_json_can_include_bounded_chunk_text() {
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

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--json",
            "--include-text",
            "--max-chars",
            "80",
            "Hybrid retrieval",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let hits: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = hits[0]["text"].as_str().unwrap();
    assert!(text.contains("Hybrid retrieval"));
    assert!(text.chars().count() <= 83);
}

#[test]
fn search_help_lists_compact_context_options() {
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--include-text"))
        .stdout(predicate::str::contains("--max-chars"));
}
