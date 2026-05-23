mod common;

use assert_cmd::Command;
use obsidian_kb::config;
use predicates::prelude::*;
use rusqlite::Connection;

#[test]
fn index_reports_counts() {
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
        .success()
        .stdout(predicate::str::contains("chunks"));
    assert!(vault.join(".obsidian-kb/metadata.sqlite").exists());
    assert!(vault.join(".obsidian-kb/tantivy").exists());
}

#[test]
fn repeated_index_is_idempotent_and_deleted_files_are_removed() {
    let (_temp, vault) = common::temp_vault();
    for _ in 0..2 {
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
    let db_path = vault.join(".obsidian-kb/metadata.sqlite");
    let before = count_table(&db_path, "files");

    std::fs::remove_file(vault.join("edge/broken-links.md")).unwrap();
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

    assert_eq!(count_table(&db_path, "files"), before - 1);
    assert_eq!(
        count_where(&db_path, "files", "rel_path = 'edge/broken-links.md'"),
        0
    );
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "search",
            "--vault",
            vault.to_str().unwrap(),
            "--mode",
            "bm25",
            "--json",
            "Missing Note",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("edge/broken-links.md").not());
}

#[test]
fn index_writes_benchmark_jsonl_when_enabled() {
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

    let log = std::fs::read_to_string(vault.join(".obsidian-kb/benchmarks.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();

    assert_eq!(record["command"], "index");
    assert_eq!(record["status"], "ok");
    assert_eq!(record["no_embeddings"], true);
    assert!(record["chunks"].as_u64().unwrap() > 0);
    assert_eq!(record["embeddings_rebuilt"], 0);
    assert!(record["total_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["load_vault_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["sqlite_replace_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["tantivy_rebuild_ms"].as_f64().unwrap() >= 0.0);
    assert!(record["phases"]["output_ms"].as_f64().unwrap() >= 0.0);
}

fn count_table(db_path: &std::path::Path, table: &str) -> usize {
    count_where(db_path, table, "1 = 1")
}

fn count_where(db_path: &std::path::Path, table: &str, predicate: &str) -> usize {
    let conn = Connection::open(db_path).unwrap();
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap() as usize
}
