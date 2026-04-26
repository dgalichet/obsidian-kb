use anyhow::{Context, Result, bail};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT, Value};
use tantivy::{Index, ReloadPolicy, TantivyDocument, Term, doc};

use crate::models::{ChunkRecord, SearchCandidate};
use crate::normalization::normalize_text;

#[derive(Clone, Copy)]
struct Fields {
    chunk_id: Field,
    file_id: Field,
    path: Field,
    path_text: Field,
    title: Field,
    heading_path: Field,
    tags: Field,
    body: Field,
}

pub fn rebuild(index_dir: &Path, chunks: &[ChunkRecord], remove_diacritics: bool) -> Result<()> {
    if index_dir.exists() {
        std::fs::remove_dir_all(index_dir)?;
    }
    std::fs::create_dir_all(index_dir)?;
    let (schema, fields) = build_schema();
    let index = Index::create_in_dir(index_dir, schema)?;
    let mut writer = index.writer(50_000_000)?;
    for chunk in chunks {
        writer.add_document(doc!(
            fields.chunk_id => chunk.chunk_id.clone(),
            fields.file_id => chunk.file_id.to_string(),
            fields.path => chunk.note_path.clone(),
            fields.path_text => normalize_text(&chunk.note_path, remove_diacritics),
            fields.title => normalize_text(&chunk.title, remove_diacritics),
            fields.heading_path => normalize_text(&chunk.heading_path, remove_diacritics),
            fields.tags => normalize_text(&chunk.tags.join(" "), remove_diacritics),
            fields.body => normalize_text(
                &format!("{}\n{}\n{}", chunk.heading_path, chunk.tags.join(" "), chunk.text),
                remove_diacritics
            ),
        ))?;
    }
    writer.commit()?;
    Ok(())
}

pub fn open_or_create(index_dir: &Path) -> Result<()> {
    if index_dir.join("meta.json").exists() {
        Index::open_in_dir(index_dir)
            .with_context(|| format!("failed to open Tantivy index: {}", index_dir.display()))?;
        return Ok(());
    }
    if index_dir.exists() && index_dir.read_dir()?.next().is_some() {
        bail!(
            "Tantivy directory exists but is not an index: {}",
            index_dir.display()
        );
    }
    std::fs::create_dir_all(index_dir)?;
    let (schema, _) = build_schema();
    Index::create_in_dir(index_dir, schema)?;
    Ok(())
}

pub fn search(
    index_dir: &Path,
    query_text: &str,
    limit: usize,
    remove_diacritics: bool,
) -> Result<Vec<SearchCandidate>> {
    let index = Index::open_in_dir(index_dir)
        .with_context(|| format!("failed to open Tantivy index: {}", index_dir.display()))?;
    let fields = fields_from_schema(index.schema())?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;
    let searcher = reader.searcher();
    let mut parser = QueryParser::for_index(
        &index,
        vec![
            fields.title,
            fields.heading_path,
            fields.tags,
            fields.body,
            fields.path_text,
        ],
    );
    parser.set_field_boost(fields.title, 3.0);
    parser.set_field_boost(fields.heading_path, 2.0);
    parser.set_field_boost(fields.tags, 1.5);
    parser.set_field_boost(fields.path_text, 0.5);
    let normalized_query = normalize_text(query_text, remove_diacritics);
    let query = match parser.parse_query(&normalized_query) {
        Ok(query) => query,
        Err(_) => {
            let fallback = safe_disjunction_query(&normalized_query);
            if fallback.is_empty() {
                return Ok(Vec::new());
            }
            match parser.parse_query(&fallback) {
                Ok(query) => query,
                Err(_) => return Ok(Vec::new()),
            }
        }
    };
    let top_docs = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;
    let mut candidates = Vec::with_capacity(top_docs.len());
    for (score, address) in top_docs {
        let document: TantivyDocument = searcher.doc(address)?;
        if let Some(chunk_id) = document
            .get_first(fields.chunk_id)
            .and_then(|value| value.as_value().as_str())
        {
            candidates.push(SearchCandidate {
                chunk_id: chunk_id.to_string(),
                score,
            });
        }
    }
    Ok(candidates)
}

#[allow(dead_code)]
pub fn delete_path(index_dir: &Path, path: &str) -> Result<()> {
    let index = Index::open_in_dir(index_dir)?;
    let fields = fields_from_schema(index.schema())?;
    let mut writer = index.writer::<TantivyDocument>(15_000_000)?;
    writer.delete_term(Term::from_field_text(fields.path, path));
    writer.commit()?;
    Ok(())
}

fn build_schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    let fields = Fields {
        chunk_id: builder.add_text_field("chunk_id", STRING | STORED),
        file_id: builder.add_text_field("file_id", STRING | STORED),
        path: builder.add_text_field("path", STRING | STORED),
        path_text: builder.add_text_field("path_text", TEXT | STORED),
        title: builder.add_text_field("title", TEXT | STORED),
        heading_path: builder.add_text_field("heading_path", TEXT | STORED),
        tags: builder.add_text_field("tags", TEXT | STORED),
        body: builder.add_text_field("body", TEXT | STORED),
    };
    (builder.build(), fields)
}

fn fields_from_schema(schema: Schema) -> Result<Fields> {
    Ok(Fields {
        chunk_id: schema.get_field("chunk_id")?,
        file_id: schema.get_field("file_id")?,
        path: schema.get_field("path")?,
        path_text: schema.get_field("path_text")?,
        title: schema.get_field("title")?,
        heading_path: schema.get_field("heading_path")?,
        tags: schema.get_field("tags")?,
        body: schema.get_field("body")?,
    })
}

fn safe_disjunction_query(query: &str) -> String {
    let tokens = query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.join(" OR ")
}
