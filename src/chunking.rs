use blake3::Hasher;

use crate::config::IndexConfig;
use crate::markdown;
use crate::models::{ChunkRecord, Heading};

#[derive(Debug, Clone)]
struct Block {
    heading_path: String,
    heading_chain: Vec<String>,
    heading_level: Option<usize>,
    text: String,
    start_line: usize,
    end_line: usize,
}

struct ChunkContext<'a> {
    note_path: &'a str,
    title: &'a str,
    tags: &'a [String],
}

pub fn chunk_markdown(
    note_path: &str,
    title: &str,
    tags: &[String],
    body: &str,
    headings: &[Heading],
    config: &IndexConfig,
) -> Vec<ChunkRecord> {
    chunk_markdown_with_body_start_line(note_path, title, tags, body, 1, headings, config)
}

/// Chunks Markdown body content that starts after earlier source-file lines.
///
/// `headings` must use source-file line numbers in the same coordinate space as
/// `body_start_line`.
pub fn chunk_markdown_with_body_start_line(
    note_path: &str,
    title: &str,
    tags: &[String],
    body: &str,
    body_start_line: usize,
    headings: &[Heading],
    config: &IndexConfig,
) -> Vec<ChunkRecord> {
    let sections = sections_by_heading(body, body_start_line, headings);
    let excluded_headings = ExcludedHeadings::new(&config.exclude_headings);
    let context = ChunkContext {
        note_path,
        title,
        tags,
    };
    let mut chunks = Vec::new();
    let mut identity_ordinal = 0usize;

    for section in sections {
        let section_chunks = chunk_blocks_for_section(&section, config);
        let section_identity_ordinal = identity_ordinal;
        identity_ordinal += section_chunks.len();
        if excluded_headings.matches(&section) {
            continue;
        }

        for (offset, chunk) in section_chunks.iter().enumerate() {
            push_chunk(
                &mut chunks,
                &context,
                section_identity_ordinal + offset,
                chunk,
            );
        }
    }

    chunks
}

fn sections_by_heading(body: &str, body_start_line: usize, headings: &[Heading]) -> Vec<Block> {
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut heading_index = 0;
    let mut current = String::new();
    let mut current_start = body_start_line;
    let mut current_heading = String::new();
    let mut current_heading_chain = Vec::new();
    let mut current_level = None;

    for (line_index, line) in body.lines().enumerate() {
        let line_no = body_start_line + line_index;
        while heading_index < headings.len() && headings[heading_index].line == line_no {
            flush_block(
                &mut sections,
                &mut current,
                current_start,
                line_no.saturating_sub(1),
                &current_heading,
                &current_heading_chain,
                current_level,
            );
            let heading = &headings[heading_index];
            heading_stack.retain(|(level, _)| *level < heading.level);
            heading_stack.push((heading.level, heading.text.clone()));
            current_heading_chain = heading_stack
                .iter()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>();
            current_heading = current_heading_chain
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" > ");
            current_level = Some(heading.level);
            current_start = line_no;
            heading_index += 1;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    let final_line = body_start_line + body.lines().count().saturating_sub(1);
    flush_block(
        &mut sections,
        &mut current,
        current_start,
        final_line,
        &current_heading,
        &current_heading_chain,
        current_level,
    );
    sections
}

fn chunk_blocks_for_section(section: &Block, config: &IndexConfig) -> Vec<Block> {
    if char_count(&section.text) <= config.max_chunk_chars {
        return vec![section.clone()];
    }
    split_long_section(section, config)
}

fn split_long_section(section: &Block, config: &IndexConfig) -> Vec<Block> {
    let blocks = paragraph_blocks(&section.text, section.start_line, &section.heading_path);
    let mut chunks = Vec::new();
    let mut current_text = String::new();
    let mut current_start = section.start_line;
    let mut current_end = section.start_line;

    for block in blocks {
        let current_chars = char_count(&current_text);
        let block_chars = char_count(&block.text);
        let would_exceed_target =
            current_chars > 0 && current_chars + block_chars > config.chunk_target_chars;
        let would_exceed_max =
            current_chars > 0 && current_chars + block_chars > config.max_chunk_chars;

        if would_exceed_target || would_exceed_max {
            push_split_block(
                &mut chunks,
                section,
                &current_text,
                current_start,
                current_end,
            );
            current_text = overlap_tail_chars(&current_text, config.chunk_overlap_chars);
            current_start = block.start_line;
        }

        if !current_text.trim().is_empty() {
            current_text.push_str("\n\n");
        }
        current_text.push_str(block.text.trim());
        current_end = block.end_line;
    }

    push_split_block(
        &mut chunks,
        section,
        &current_text,
        current_start,
        current_end,
    );
    chunks
}

fn push_split_block(
    chunks: &mut Vec<Block>,
    section: &Block,
    text: &str,
    start_line: usize,
    end_line: usize,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    chunks.push(Block {
        heading_path: section.heading_path.clone(),
        heading_chain: section.heading_chain.clone(),
        heading_level: section.heading_level,
        text: text.to_string(),
        start_line,
        end_line,
    });
}

fn paragraph_blocks(text: &str, start_line: usize, heading_path: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut current_start = start_line;
    let mut in_code = false;

    for (offset, line) in text.lines().enumerate() {
        let line_no = start_line + offset;
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
        }
        if line.trim().is_empty() && !in_code {
            flush_block(
                &mut blocks,
                &mut current,
                current_start,
                line_no,
                heading_path,
                &[],
                None,
            );
            current_start = line_no + 1;
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    let final_line = start_line + text.lines().count().saturating_sub(1);
    flush_block(
        &mut blocks,
        &mut current,
        current_start,
        final_line.max(start_line),
        heading_path,
        &[],
        None,
    );
    blocks
}

fn flush_block(
    blocks: &mut Vec<Block>,
    current: &mut String,
    start_line: usize,
    end_line: usize,
    heading_path: &str,
    heading_chain: &[String],
    heading_level: Option<usize>,
) {
    let text = current.trim();
    if !text.is_empty() {
        blocks.push(Block {
            heading_path: heading_path.to_string(),
            heading_chain: heading_chain.to_vec(),
            heading_level,
            text: text.to_string(),
            start_line,
            end_line: end_line.max(start_line),
        });
    }
    current.clear();
}

struct ExcludedHeadings {
    patterns: Vec<String>,
}

impl ExcludedHeadings {
    fn new(values: &[String]) -> Self {
        let patterns = values
            .iter()
            .map(|value| normalize_heading_match(value))
            .filter(|value| !value.is_empty())
            .collect();
        Self { patterns }
    }

    fn matches(&self, section: &Block) -> bool {
        if self.patterns.is_empty() || section.heading_path.is_empty() {
            return false;
        }
        let path = normalize_heading_match(&section.heading_path);
        let chain = section
            .heading_chain
            .iter()
            .map(|heading| normalize_heading_match(heading))
            .collect::<Vec<_>>();

        self.patterns.iter().any(|pattern| {
            let pattern = pattern.as_str();
            path == pattern
                || path
                    .strip_prefix(pattern)
                    .is_some_and(|suffix| suffix.starts_with(" > "))
                || (!pattern.contains('>') && chain.iter().any(|heading| heading == pattern))
        })
    }
}

fn normalize_heading_match(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn push_chunk(
    chunks: &mut Vec<ChunkRecord>,
    context: &ChunkContext<'_>,
    identity_ordinal: usize,
    block: &Block,
) {
    let trimmed = block.text.trim();
    if trimmed.is_empty() {
        return;
    }
    let ordinal = chunks.len();
    let text_hash = blake3::hash(trimmed.as_bytes()).to_hex().to_string();
    let mut hasher = Hasher::new();
    hasher.update(context.note_path.as_bytes());
    hasher.update(block.heading_path.as_bytes());
    hasher.update(&identity_ordinal.to_le_bytes());
    hasher.update(text_hash.as_bytes());
    let chunk_id = hasher.finalize().to_hex().to_string();
    chunks.push(ChunkRecord {
        file_id: 0,
        chunk_id,
        note_path: context.note_path.to_string(),
        title: context.title.to_string(),
        ordinal,
        heading_path: block.heading_path.clone(),
        heading_level: block.heading_level,
        text: trimmed.to_string(),
        start_line: block.start_line,
        end_line: block.end_line,
        text_hash,
        tags: context.tags.to_vec(),
    });
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn overlap_tail_chars(value: &str, chars: usize) -> String {
    if chars == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= chars {
        return String::new();
    }
    value.chars().skip(count - chars).collect()
}

#[allow(dead_code)]
fn _strip_for_chunks(value: &str) -> String {
    markdown::strip_fenced_code(value)
}
