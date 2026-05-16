mod common;

use assert_cmd::Command;
use obsidian_kb::{config, models::SearchHit};
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
fn graph_search_honors_configured_depth() {
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
            "--expand-graph",
            "--top",
            "20",
            "--json",
            "mesh namespace routing",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let hits: Vec<SearchHit> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(hits.iter().any(|hit| hit.path == "aws/app-mesh.md"));
    assert!(
        !hits
            .iter()
            .any(|hit| hit.path == "ai/lost-in-the-middle.md")
    );

    let config_path = vault.join(".obsidian-kb.toml");
    let mut app_config = config::load_existing(None, Some(&config_path)).unwrap();
    app_config.search.graph_depth = 2;
    config::save_config(&app_config).unwrap();

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--expand-graph",
            "--top",
            "20",
            "--json",
            "mesh namespace routing",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let hits: Vec<SearchHit> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        hits.iter()
            .any(|hit| hit.path == "ai/lost-in-the-middle.md" && hit.graph)
    );
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
    assert!(hits[0]["start_line"].as_u64().unwrap() >= 1);
    assert!(hits[0]["end_line"].as_u64().unwrap() >= hits[0]["start_line"].as_u64().unwrap());
    assert!(
        hits[0]["tags"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tag| tag.is_string())
    );
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
fn search_can_filter_by_tag_without_query() {
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
            "--tag",
            "business",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let hits: Vec<SearchHit> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].path, "business/opportunities.md");
    assert_eq!(hits[0].source, "filter");
    assert!(
        hits.iter()
            .all(|hit| hit.tags.contains(&"business".to_string()))
    );
}

#[test]
fn search_filters_bm25_results_by_tag_and_property() {
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
            "--tag",
            "retrieval/hybrid",
            "--property",
            "status=active",
            "hybrid retrieval",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let hits: Vec<SearchHit> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit.path == "ai/rag.md"));
}

#[test]
fn search_property_filters_support_comparison_and_not_equal() {
    let (_temp, vault) = common::temp_vault();
    std::fs::write(
        vault.join("metadata-active.md"),
        "---\ntitle: Active Metadata\nstatus: active\npriority: 3\ncreated: 2026-01-02\n---\n# Active Metadata\nMetadata command test note.\n",
    )
    .unwrap();
    std::fs::write(
        vault.join("metadata-archived.md"),
        "---\ntitle: Archived Metadata\nstatus: archived\npriority: 1\ncreated: 2024-01-02\n---\n# Archived Metadata\nMetadata command test note.\n",
    )
    .unwrap();
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
            "--property",
            "priority>=2",
            "--property",
            "status!=archived",
            "--property",
            "created>=2026-01-01",
            "metadata command test",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let hits: Vec<SearchHit> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit.path == "metadata-active.md"));
}

#[test]
fn search_writes_benchmark_jsonl_without_query_by_default() {
    let (_temp, vault) = common::temp_vault();
    let mut app_config = config::AppConfig::default_for_vault_in(&vault, None, &vault).unwrap();
    app_config.benchmark.enabled = true;
    config::save_config(&app_config).unwrap();

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

    let log_path = vault.join(".obsidian-kb/benchmarks.jsonl");
    std::fs::remove_file(&log_path).unwrap();

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--top",
            "5",
            "mesh namespace routing",
        ])
        .assert()
        .success();

    let log = std::fs::read_to_string(log_path).unwrap();
    let record: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();

    assert_eq!(record["command"], "search");
    assert_eq!(record["status"], "ok");
    assert_eq!(record["mode"], "bm25");
    assert_eq!(record["top"], 5);
    assert_eq!(record["expand_graph"], false);
    assert_eq!(record["query_chars"], 22);
    assert!(record.get("query").is_none());
    assert!(record["phases"]["open_db_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["bm25_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["fusion_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["hydrate_results_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["output_ms"].as_f64().unwrap() >= 0.0);
}

#[test]
fn search_help_lists_compact_context_options() {
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--tag"))
        .stdout(predicate::str::contains("--property"))
        .stdout(predicate::str::contains("--include-text"))
        .stdout(predicate::str::contains("--max-chars"));
}
