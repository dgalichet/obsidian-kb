use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::cli::SearchModeArg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Bm25,
    Vector,
    Hybrid,
}

impl From<SearchModeArg> for SearchMode {
    fn from(value: SearchModeArg) -> Self {
        match value {
            SearchModeArg::Bm25 => Self::Bm25,
            SearchModeArg::Vector => Self::Vector,
            SearchModeArg::Hybrid => Self::Hybrid,
        }
    }
}

impl SearchMode {
    pub fn from_config_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "bm25" => Some(Self::Bm25),
            "vector" => Some(Self::Vector),
            "hybrid" => Some(Self::Hybrid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedNote {
    pub path: String,
    pub absolute_path: PathBuf,
    pub title: String,
    pub folder: String,
    pub hash: String,
    pub mtime: i64,
    pub size: u64,
    pub frontmatter: Value,
    pub body: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<WikiLink>,
    pub headings: Vec<Heading>,
    pub chunks: Vec<ChunkRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WikiLink {
    pub raw: String,
    pub target: String,
    pub anchor: Option<String>,
    pub display: Option<String>,
    pub embedded: bool,
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line: usize,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkRecord {
    pub file_id: i64,
    pub chunk_id: String,
    pub note_path: String,
    pub title: String,
    pub ordinal: usize,
    pub heading_path: String,
    pub heading_level: Option<usize>,
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text_hash: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteSummary {
    pub path: String,
    pub title: String,
    pub folder: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndexStats {
    pub notes: usize,
    pub changed_files: usize,
    pub unchanged_files: usize,
    pub deleted_files: usize,
    pub chunks: usize,
    pub aliases: usize,
    pub tags: usize,
    pub links: usize,
    pub unresolved_links: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub final_rank: usize,
    pub final_score: f32,
    pub path: String,
    pub title: String,
    pub chunk_id: String,
    pub heading_path: String,
    pub heading: String,
    pub start_line: usize,
    pub end_line: usize,
    pub tags: Vec<String>,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub bm25_rank: Option<usize>,
    pub bm25_score: Option<f32>,
    pub vector_rank: Option<usize>,
    pub vector_score: Option<f32>,
    pub graph_boost: f32,
    pub score: f32,
    pub source: String,
    pub lexical_rank: Option<usize>,
    pub graph: bool,
}

#[derive(Debug, Clone)]
pub struct SearchCandidate {
    pub chunk_id: String,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct GraphReport {
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedLinkRecord {
    pub source_path: String,
    pub target_raw: String,
    pub target_normalized: String,
    pub link_type: String,
    pub link_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphView {
    pub root: NoteSummary,
    pub depth: usize,
    pub nodes: Vec<NoteSummary>,
    pub neighboring_notes: Vec<NoteSummary>,
    pub outgoing_links: Vec<GraphEdge>,
    pub backlinks: Vec<GraphEdge>,
    pub unresolved_links: Vec<String>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsReport {
    pub notes: usize,
    pub chunks: usize,
    pub aliases: usize,
    pub tags: usize,
    pub links: usize,
    pub unresolved_links: usize,
    pub embeddings: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub version: String,
    pub config_path: String,
    pub vault_path: String,
    pub index_dir: String,
    pub markdown_files: usize,
    pub indexed_files: usize,
    pub embeddings: usize,
    pub issue_count: usize,
    pub fatal_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub notes: usize,
    pub chunks: usize,
    pub checks: Vec<DoctorCheck>,
    pub unresolved_link_groups: Vec<DoctorLinkGroup>,
    pub issues: Vec<DoctorIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorIssue {
    pub level: String,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorLinkGroup {
    pub target: String,
    pub category: String,
    pub occurrences: usize,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}
