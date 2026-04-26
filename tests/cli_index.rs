mod common;

use assert_cmd::Command;
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
