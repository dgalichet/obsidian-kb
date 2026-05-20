mod common;

use assert_cmd::Command;
use rusqlite::Connection;
use std::collections::BTreeSet;
use tantivy::Index;

#[test]
fn index_storage_matches_sqlite_and_tantivy_contract() {
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

    let db = Connection::open(vault.join(".obsidian-kb/metadata.sqlite")).unwrap();
    assert_table_columns(
        &db,
        "files",
        &[
            "id",
            "path",
            "rel_path",
            "document_kind",
            "title",
            "folder",
            "mtime_ns",
            "size_bytes",
            "content_hash",
            "frontmatter_json",
            "indexed_at",
        ],
    );
    assert_table_columns(
        &db,
        "chunks",
        &[
            "id",
            "file_id",
            "chunk_index",
            "title",
            "heading_path",
            "heading_level",
            "content",
            "content_hash",
            "start_line",
            "end_line",
            "start_page",
            "end_page",
        ],
    );
    assert_table_columns(
        &db,
        "embeddings",
        &["chunk_id", "model", "dim", "embedding", "content_hash"],
    );
    assert_table_columns(
        &db,
        "links",
        &[
            "id",
            "source_file_id",
            "target_raw",
            "target_normalized",
            "target_file_id",
            "link_text",
            "link_type",
        ],
    );
    assert_table_columns(&db, "tags", &["file_id", "tag"]);
    assert_table_columns(
        &db,
        "properties",
        &[
            "file_id",
            "key",
            "value_text",
            "value_norm",
            "value_type",
            "value_json",
        ],
    );
    assert_table_columns(&db, "aliases", &["file_id", "alias"]);
    assert_table_columns(&db, "meta", &["key", "value"]);

    let index = Index::open_in_dir(vault.join(".obsidian-kb/tantivy")).unwrap();
    let field_names = index
        .schema()
        .fields()
        .map(|(_, entry)| entry.name().to_string())
        .collect::<BTreeSet<_>>();
    for field in [
        "chunk_id",
        "file_id",
        "path",
        "title",
        "heading_path",
        "tags",
        "body",
    ] {
        assert!(field_names.contains(field), "missing Tantivy field {field}");
    }
}

fn assert_table_columns(db: &Connection, table: &str, expected: &[&str]) {
    let columns = table_columns(db, table);
    assert_eq!(columns, expected, "unexpected columns for {table}");
}

fn table_columns(db: &Connection, table: &str) -> Vec<String> {
    let mut stmt = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare pragma");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect columns")
}
