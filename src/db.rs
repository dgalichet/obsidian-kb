use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use crate::models::{
    ChunkRecord, GraphEdge, GraphView, IndexStats, NoteSummary, ParsedNote, StatsReport,
    UnresolvedLinkRecord,
};
use crate::schema;

pub struct Db {
    conn: Connection,
}

struct PreservedEmbedding {
    chunk_id: String,
    model: String,
    dim: i64,
    embedding: Vec<u8>,
    content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub mtime_ns: i64,
    pub size_bytes: i64,
    pub content_hash: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn file_snapshots(&self) -> Result<BTreeMap<String, FileSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT rel_path, IFNULL(mtime_ns, 0), IFNULL(size_bytes, 0), content_hash
             FROM files",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                FileSnapshot {
                    mtime_ns: row.get(1)?,
                    size_bytes: row.get(2)?,
                    content_hash: row.get(3)?,
                },
            ))
        })?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(Into::into)
    }

    pub fn replace_index(
        &mut self,
        notes: &[ParsedNote],
        graph_warnings: &[String],
    ) -> Result<IndexStats> {
        let preserved_embeddings = self.preserved_embeddings()?;
        let tx = self.conn.transaction()?;
        tx.execute("PRAGMA foreign_keys = ON", [])?;
        tx.execute("DELETE FROM links", [])?;
        tx.execute("DELETE FROM tags", [])?;
        tx.execute("DELETE FROM aliases", [])?;
        tx.execute("DELETE FROM chunks", [])?;
        tx.execute("DELETE FROM files", [])?;

        let indexed_at = Utc::now().to_rfc3339();
        let mut file_ids = BTreeMap::new();
        let mut chunk_hashes = BTreeMap::new();
        let mut stats = IndexStats::default();

        for note in notes {
            tx.execute(
                "INSERT INTO files(path, rel_path, title, folder, mtime_ns, size_bytes, content_hash, frontmatter_json, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    note.absolute_path.to_string_lossy(),
                    note.path,
                    note.title,
                    empty_as_none(&note.folder),
                    note.mtime,
                    note.size as i64,
                    note.hash,
                    serde_json::to_string(&note.frontmatter)?,
                    indexed_at
                ],
            )?;
            let file_id = tx.last_insert_rowid();
            file_ids.insert(note.path.clone(), file_id);
            stats.notes += 1;
        }

        for note in notes {
            let file_id = file_ids[&note.path];
            for chunk in &note.chunks {
                tx.execute(
                    "INSERT INTO chunks(id, file_id, chunk_index, title, heading_path, heading_level, content, content_hash, start_line, end_line)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        chunk.chunk_id,
                        file_id,
                        chunk.ordinal as i64,
                        chunk.title,
                        empty_as_none(&chunk.heading_path),
                        chunk.heading_level.map(|level| level as i64),
                        chunk.text,
                        chunk.text_hash,
                        chunk.start_line as i64,
                        chunk.end_line as i64
                    ],
                )?;
                chunk_hashes.insert(chunk.chunk_id.clone(), chunk.text_hash.clone());
                stats.chunks += 1;
            }

            for alias in &note.aliases {
                tx.execute(
                    "INSERT INTO aliases(file_id, alias) VALUES (?1, ?2)",
                    params![file_id, alias],
                )?;
                stats.aliases += 1;
            }

            for tag in &note.tags {
                tx.execute(
                    "INSERT INTO tags(file_id, tag) VALUES (?1, ?2)",
                    params![file_id, tag],
                )?;
                stats.tags += 1;
            }

            for link in &note.links {
                let target_file_id = link
                    .target_path
                    .as_ref()
                    .and_then(|path| file_ids.get(path))
                    .copied();
                tx.execute(
                    "INSERT INTO links(source_file_id, target_raw, target_normalized, target_file_id, link_text, link_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        file_id,
                        link.raw,
                        normalize_target(&link.target),
                        target_file_id,
                        link.display,
                        if link.embedded { "embedded" } else { "wikilink" }
                    ],
                )?;
                stats.links += 1;
                if target_file_id.is_none() {
                    stats.unresolved_links += 1;
                }
            }

            stats.warnings += note.warnings.len();
        }

        stats.warnings += graph_warnings.len();
        let warnings = notes
            .iter()
            .flat_map(|note| note.warnings.iter().cloned())
            .chain(graph_warnings.iter().cloned())
            .collect::<Vec<_>>();
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', '2')",
            [],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('warnings_json', ?1)",
            params![serde_json::to_string(&warnings)?],
        )?;
        for embedding in preserved_embeddings {
            if chunk_hashes.get(&embedding.chunk_id) == Some(&embedding.content_hash) {
                tx.execute(
                    "INSERT OR REPLACE INTO embeddings(chunk_id, model, dim, embedding, content_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        embedding.chunk_id,
                        embedding.model,
                        embedding.dim,
                        embedding.embedding,
                        embedding.content_hash
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    pub fn load_all_chunks(&self) -> Result<Vec<ChunkRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "{CHUNK_SELECT_BASE} ORDER BY f.rel_path, c.chunk_index"
        ))?;
        let rows = stmt.query_map([], chunk_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn load_chunk(&self, chunk_id: &str) -> Result<Option<ChunkRecord>> {
        self.conn
            .query_row(
                &format!("{CHUNK_SELECT_BASE} WHERE c.id = ?1"),
                params![chunk_id],
                chunk_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn first_chunk_for_note(&self, path: &str) -> Result<Option<ChunkRecord>> {
        self.conn
            .query_row(
                &format!(
                    "{CHUNK_SELECT_BASE} WHERE f.rel_path = ?1 ORDER BY c.chunk_index LIMIT 1"
                ),
                params![path],
                chunk_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn neighbor_paths(&self, path: &str, limit: usize) -> Result<Vec<String>> {
        let Some(file_id) = self.file_id_for_rel_path(path)? else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT rel_path
             FROM (
                SELECT DISTINCT target.rel_path AS rel_path
                FROM links
                JOIN files target ON target.id = links.target_file_id
                WHERE links.source_file_id = ?1
                UNION
                SELECT DISTINCT source.rel_path AS rel_path
                FROM links
                JOIN files source ON source.id = links.source_file_id
                WHERE links.target_file_id = ?1
             )
             ORDER BY rel_path
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![file_id, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn resolve_note_path(&self, identifier: &str) -> Result<Option<String>> {
        let normalized = normalize(identifier);
        let path_candidate = normalize_path(identifier);
        let path_with_extension = if path_candidate.ends_with(".md") {
            path_candidate.clone()
        } else {
            format!("{path_candidate}.md")
        };

        self.conn
            .query_row(
                "SELECT rel_path
                 FROM (
                    SELECT rel_path, 0 AS rank FROM files WHERE lower(rel_path) = ?1
                    UNION ALL
                    SELECT rel_path, 1 AS rank FROM files WHERE lower(rel_path) = ?2
                    UNION ALL
                    SELECT rel_path, 2 AS rank FROM files WHERE lower(title) = ?3
                    UNION ALL
                    SELECT files.rel_path, 3 AS rank
                    FROM aliases
                    JOIN files ON files.id = aliases.file_id
                    WHERE lower(aliases.alias) = ?3
                 )
                 ORDER BY rank, rel_path
                 LIMIT 1",
                params![path_candidate, path_with_extension, normalized],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn note_summary(&self, path: &str) -> Result<Option<NoteSummary>> {
        self.conn
            .query_row(
                "SELECT f.rel_path, f.title, IFNULL(f.folder, ''),
                        IFNULL((SELECT group_concat(alias, char(31)) FROM aliases WHERE file_id = f.id), ''),
                        IFNULL((SELECT group_concat(tag, char(31)) FROM tags WHERE file_id = f.id), '')
                 FROM files f
                 WHERE f.rel_path = ?1",
                params![path],
                note_summary_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn graph_view(&self, identifier: &str, depth: usize) -> Result<Option<GraphView>> {
        let Some(root_path) = self.resolve_note_path(identifier)? else {
            return Ok(None);
        };
        let Some(root) = self.note_summary(&root_path)? else {
            return Ok(None);
        };
        let outgoing_links = self.outgoing_edges_for_note(&root_path)?;
        let backlinks = self.backlink_edges_for_note(&root_path)?;
        let unresolved_links = self.unresolved_links_for_note(&root_path)?;

        let mut queue = VecDeque::from([(root_path.clone(), 0usize)]);
        let mut seen = BTreeSet::from([root_path.clone()]);
        let mut edges = BTreeSet::new();

        while let Some((path, current_depth)) = queue.pop_front() {
            if current_depth >= depth {
                continue;
            }
            for edge in self.edges_for_note(&path)? {
                let neighbor = if edge.source == path {
                    edge.target.clone()
                } else {
                    edge.source.clone()
                };
                if seen.insert(neighbor.clone()) {
                    queue.push_back((neighbor, current_depth + 1));
                }
                edges.insert(edge);
            }
        }

        let mut nodes = Vec::new();
        for path in seen {
            if let Some(summary) = self.note_summary(&path)? {
                nodes.push(summary);
            }
        }
        nodes.sort_by(|left, right| left.path.cmp(&right.path));
        let neighboring_notes = nodes
            .iter()
            .filter(|node| node.path != root.path)
            .cloned()
            .collect();

        Ok(Some(GraphView {
            root,
            depth,
            nodes,
            neighboring_notes,
            outgoing_links,
            backlinks,
            unresolved_links,
            edges: edges.into_iter().collect(),
        }))
    }

    pub fn stats(&self) -> Result<StatsReport> {
        Ok(StatsReport {
            notes: self.count_table("files")?,
            chunks: self.count_table("chunks")?,
            aliases: self.count_table("aliases")?,
            tags: self.count_table("tags")?,
            links: self.count_table("links")?,
            unresolved_links: self.count_where("links", "target_file_id IS NULL")?,
            embeddings: self.count_table("embeddings")?,
            warnings: self.warnings()?.len(),
        })
    }

    pub fn counts(&self) -> Result<(usize, usize)> {
        Ok((self.count_table("files")?, self.count_table("chunks")?))
    }

    pub fn duplicate_aliases(&self) -> Result<Vec<(String, Vec<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT lower(a.alias), group_concat(DISTINCT f.rel_path)
             FROM aliases a
             JOIN files f ON f.id = a.file_id
             GROUP BY lower(a.alias)
             HAVING COUNT(DISTINCT a.file_id) > 1
             ORDER BY lower(a.alias)",
        )?;
        let rows = stmt.query_map([], |row| {
            let alias: String = row.get(0)?;
            let paths: String = row.get(1)?;
            Ok((alias, paths.split(',').map(ToOwned::to_owned).collect()))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn unresolved_links(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.rel_path, links.target_raw
             FROM links
             JOIN files f ON f.id = links.source_file_id
             WHERE links.target_file_id IS NULL
             ORDER BY f.rel_path, links.target_raw",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn unresolved_link_records(&self) -> Result<Vec<UnresolvedLinkRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.rel_path, links.target_raw, links.target_normalized, links.link_type, links.link_text
             FROM links
             JOIN files f ON f.id = links.source_file_id
             WHERE links.target_file_id IS NULL
             ORDER BY links.target_normalized, f.rel_path, links.target_raw",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(UnresolvedLinkRecord {
                source_path: row.get(0)?,
                target_raw: row.get(1)?,
                target_normalized: row.get(2)?,
                link_type: row.get(3)?,
                link_text: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn unresolved_links_for_note(&self, path: &str) -> Result<Vec<String>> {
        let Some(file_id) = self.file_id_for_rel_path(path)? else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT target_raw
             FROM links
             WHERE source_file_id = ?1 AND target_file_id IS NULL
             ORDER BY target_raw",
        )?;
        let rows = stmt.query_map(params![file_id], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn warnings(&self) -> Result<Vec<String>> {
        let value = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'warnings_json'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "[]".to_string());
        serde_json::from_str(&value).map_err(Into::into)
    }

    pub fn insert_embedding(
        &self,
        chunk_id: &str,
        model: &str,
        dim: usize,
        vector: &[u8],
        content_hash: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings(chunk_id, model, dim, embedding, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![chunk_id, model, dim as i64, vector, content_hash],
        )?;
        Ok(())
    }

    pub fn load_embeddings(&self, model: &str) -> Result<Vec<(String, usize, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, dim, embedding
             FROM embeddings
             WHERE model = ?1",
        )?;
        let rows = stmt.query_map(params![model], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn load_embedding_hashes(&self, model: &str) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, content_hash
             FROM embeddings
             WHERE model = ?1",
        )?;
        let rows = stmt.query_map(params![model], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(Into::into)
    }

    fn preserved_embeddings(&self) -> Result<Vec<PreservedEmbedding>> {
        let mut stmt = self.conn.prepare(
            "SELECT chunk_id, model, dim, embedding, content_hash
             FROM embeddings",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PreservedEmbedding {
                chunk_id: row.get(0)?,
                model: row.get(1)?,
                dim: row.get(2)?,
                embedding: row.get(3)?,
                content_hash: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn edges_for_note(&self, path: &str) -> Result<Vec<GraphEdge>> {
        let mut edges = self.outgoing_edges_for_note(path)?;
        edges.extend(self.backlink_edges_for_note(path)?);
        Ok(edges)
    }

    fn outgoing_edges_for_note(&self, path: &str) -> Result<Vec<GraphEdge>> {
        let Some(file_id) = self.file_id_for_rel_path(path)? else {
            return Ok(Vec::new());
        };
        let mut outgoing = self.conn.prepare(
            "SELECT source.rel_path, target.rel_path, links.target_raw
             FROM links
             JOIN files source ON source.id = links.source_file_id
             JOIN files target ON target.id = links.target_file_id
             WHERE links.source_file_id = ?1
             ORDER BY target.rel_path, links.target_raw",
        )?;
        let rows = outgoing.query_map(params![file_id], |row| {
            Ok(GraphEdge {
                source: row.get(0)?,
                target: row.get(1)?,
                kind: "outgoing".to_string(),
                raw: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn backlink_edges_for_note(&self, path: &str) -> Result<Vec<GraphEdge>> {
        let Some(file_id) = self.file_id_for_rel_path(path)? else {
            return Ok(Vec::new());
        };
        let mut backlinks = self.conn.prepare(
            "SELECT source.rel_path, target.rel_path, links.target_raw
             FROM links
             JOIN files source ON source.id = links.source_file_id
             JOIN files target ON target.id = links.target_file_id
             WHERE links.target_file_id = ?1
             ORDER BY source.rel_path, links.target_raw",
        )?;
        let rows = backlinks.query_map(params![file_id], |row| {
            Ok(GraphEdge {
                source: row.get(0)?,
                target: row.get(1)?,
                kind: "backlink".to_string(),
                raw: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn file_id_for_rel_path(&self, path: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM files WHERE rel_path = ?1",
                params![path],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    fn count_table(&self, table: &str) -> Result<usize> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let count = self.conn.query_row(&sql, [], |row| row.get::<_, i64>(0))?;
        Ok(count as usize)
    }

    fn count_where(&self, table: &str, predicate: &str) -> Result<usize> {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
        let count = self.conn.query_row(&sql, [], |row| row.get::<_, i64>(0))?;
        Ok(count as usize)
    }
}

const CHUNK_SELECT_BASE: &str = "\
    SELECT c.id, c.file_id, f.rel_path, c.title, c.chunk_index, IFNULL(c.heading_path, ''),
           c.heading_level, c.content, c.start_line, c.end_line, c.content_hash,
           IFNULL((SELECT group_concat(tag, ' ') FROM tags WHERE file_id = c.file_id), '')
    FROM chunks c
    JOIN files f ON f.id = c.file_id";

fn chunk_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkRecord> {
    let tags: String = row.get(11)?;
    let tags = tags
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(ChunkRecord {
        chunk_id: row.get(0)?,
        file_id: row.get(1)?,
        note_path: row.get(2)?,
        title: row.get(3)?,
        ordinal: row.get::<_, i64>(4)? as usize,
        heading_path: row.get(5)?,
        heading_level: row.get::<_, Option<i64>>(6)?.map(|level| level as usize),
        text: row.get(7)?,
        start_line: row.get::<_, Option<i64>>(8)?.unwrap_or_default() as usize,
        end_line: row.get::<_, Option<i64>>(9)?.unwrap_or_default() as usize,
        text_hash: row.get(10)?,
        tags,
    })
}

fn note_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NoteSummary> {
    let aliases: String = row.get(3)?;
    let tags: String = row.get(4)?;
    Ok(NoteSummary {
        path: row.get(0)?,
        title: row.get(1)?,
        folder: row.get(2)?,
        aliases: split_unit_separator(&aliases),
        tags: split_unit_separator(&tags),
    })
}

fn empty_as_none(value: &str) -> Option<&str> {
    if value.is_empty() { None } else { Some(value) }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn normalize_path(value: &str) -> String {
    normalize(value).replace('\\', "/")
}

fn normalize_target(value: &str) -> String {
    normalize_path(value.trim_end_matches(".md"))
}

fn split_unit_separator(value: &str) -> Vec<String> {
    value
        .split('\u{1f}')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
