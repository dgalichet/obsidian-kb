mod common;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn show_graph_and_stats_support_json() {
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
            "mesh namespace routing",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let hits: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let chunk_id = hits[0]["best_chunk_id"].as_str().unwrap();

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "show",
            "--vault",
            vault.to_str().unwrap(),
            chunk_id,
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"note_path\""));

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--json",
            "--top",
            "2",
            "note",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let hits: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let first_chunk_id = hits[0]["best_chunk_id"].as_str().unwrap();
    let second_chunk_id = hits[1]["best_chunk_id"].as_str().unwrap();
    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "show",
            "--vault",
            vault.to_str().unwrap(),
            first_chunk_id,
            second_chunk_id,
            "missing-chunk",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["chunks"].as_array().unwrap().len(), 2);
    assert_eq!(report["missing"][0], "missing-chunk");

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "graph",
            "--vault",
            vault.to_str().unwrap(),
            "ECS Service Connect",
            "--depth",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("aws/app-mesh.md"));

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["stats", "--vault", vault.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"notes\""));

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "tags",
            "--vault",
            vault.to_str().unwrap(),
            "--prefix",
            "business",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"tag\": \"business\""));

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["properties", "--vault", vault.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"status\""));

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "properties",
            "--vault",
            vault.to_str().unwrap(),
            "--key",
            "status",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"value\": \"active\""));
}
