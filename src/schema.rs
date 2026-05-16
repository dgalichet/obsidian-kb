use rusqlite::{Connection, Result};

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    if legacy_schema_present(conn)? {
        drop_app_tables(conn)?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL,
            rel_path TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            folder TEXT,
            mtime_ns INTEGER,
            size_bytes INTEGER,
            content_hash TEXT NOT NULL,
            frontmatter_json TEXT,
            indexed_at TEXT
        );

        CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            chunk_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            heading_path TEXT,
            heading_level INTEGER,
            content TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            start_line INTEGER,
            end_line INTEGER
        );

        CREATE TABLE IF NOT EXISTS embeddings (
            chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            model TEXT NOT NULL,
            dim INTEGER NOT NULL,
            embedding BLOB NOT NULL,
            content_hash TEXT NOT NULL,
            PRIMARY KEY(chunk_id, model)
        );

        CREATE TABLE IF NOT EXISTS links (
            id INTEGER PRIMARY KEY,
            source_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            target_raw TEXT NOT NULL,
            target_normalized TEXT NOT NULL,
            target_file_id INTEGER REFERENCES files(id) ON DELETE SET NULL,
            link_text TEXT,
            link_type TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tags (
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            tag TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS properties (
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_text TEXT NOT NULL,
            value_norm TEXT NOT NULL,
            value_type TEXT NOT NULL,
            value_json TEXT
        );

        CREATE TABLE IF NOT EXISTS aliases (
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            alias TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_files_rel_path ON files(rel_path);
        CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);
        CREATE INDEX IF NOT EXISTS idx_links_source_file_id ON links(source_file_id);
        CREATE INDEX IF NOT EXISTS idx_links_target_file_id ON links(target_file_id);
        CREATE INDEX IF NOT EXISTS idx_tags_file_id ON tags(file_id);
        CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag);
        CREATE INDEX IF NOT EXISTS idx_properties_file_id ON properties(file_id);
        CREATE INDEX IF NOT EXISTS idx_properties_key_value ON properties(key, value_norm);
        CREATE INDEX IF NOT EXISTS idx_aliases_file_id ON aliases(file_id);
        CREATE INDEX IF NOT EXISTS idx_aliases_alias ON aliases(alias);
        "#,
    )
}

fn legacy_schema_present(conn: &Connection) -> Result<bool> {
    let has_notes = table_exists(conn, "notes")?;
    let has_files = table_exists(conn, "files")?;
    let has_old_chunks = if table_exists(conn, "chunks")? {
        !column_exists(conn, "chunks", "file_id")?
    } else {
        false
    };
    Ok(has_notes || has_old_chunks && !has_files)
}

fn drop_app_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        DROP TABLE IF EXISTS warnings;
        DROP TABLE IF EXISTS embeddings;
        DROP TABLE IF EXISTS links;
        DROP TABLE IF EXISTS tags;
        DROP TABLE IF EXISTS properties;
        DROP TABLE IF EXISTS aliases;
        DROP TABLE IF EXISTS chunks;
        DROP TABLE IF EXISTS notes;
        DROP TABLE IF EXISTS files;
        DROP TABLE IF EXISTS meta;
        "#,
    )
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}
