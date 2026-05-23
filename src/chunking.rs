use blake3::Hasher;

use crate::config::IndexConfig;
use crate::markdown;
use crate::models::{ChunkRecord, DocumentKind, Heading};

#[derive(Debug, Clone)]
struct Block {
    heading_path: String,
    heading_chain: Vec<String>,
    heading_level: Option<usize>,
    text: String,
    start_line: usize,
    end_line: usize,
    start_page: Option<usize>,
    end_page: Option<usize>,
}

struct ChunkContext<'a> {
    note_path: &'a str,
    document_kind: DocumentKind,
    title: &'a str,
    tags: &'a [String],
}

#[derive(Debug, Clone, Copy)]
struct BlockLocation {
    start_line: usize,
    end_line: usize,
    start_page: Option<usize>,
    end_page: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct TextSection {
    pub heading_path: String,
    pub heading_level: Option<usize>,
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_page: Option<usize>,
    pub end_page: Option<usize>,
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
        document_kind: DocumentKind::Markdown,
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

pub fn chunk_text_sections(
    note_path: &str,
    document_kind: DocumentKind,
    title: &str,
    tags: &[String],
    sections: &[TextSection],
    config: &IndexConfig,
) -> Vec<ChunkRecord> {
    let context = ChunkContext {
        note_path,
        document_kind,
        title,
        tags,
    };
    let mut chunks = Vec::new();
    let mut identity_ordinal = 0usize;

    for section in sections {
        let section = Block {
            heading_path: section.heading_path.clone(),
            heading_chain: Vec::new(),
            heading_level: section.heading_level,
            text: section.text.clone(),
            start_line: section.start_line,
            end_line: section.end_line,
            start_page: section.start_page,
            end_page: section.end_page,
        };
        let section_chunks = chunk_blocks_for_section(&section, config);
        let section_identity_ordinal = identity_ordinal;
        identity_ordinal += section_chunks.len();
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
                BlockLocation {
                    start_line: current_start,
                    end_line: line_no.saturating_sub(1),
                    start_page: None,
                    end_page: None,
                },
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
        BlockLocation {
            start_line: current_start,
            end_line: final_line,
            start_page: None,
            end_page: None,
        },
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
    let paragraph_blocks = paragraph_blocks(
        &section.text,
        section.start_line,
        &section.heading_path,
        section.start_page,
        section.end_page,
    );
    let mut chunks = Vec::new();
    let mut current_text = String::new();
    let mut current_start = section.start_line;
    let mut current_end = section.start_line;

    for block in paragraph_blocks
        .into_iter()
        .flat_map(|block| split_oversized_block(block, config.max_chunk_chars))
    {
        let current_chars = char_count(&current_text);
        let block_chars = char_count(&block.text);
        let separator_chars = if current_text.trim().is_empty() { 0 } else { 2 };
        let combined_chars = current_chars + separator_chars + block_chars;
        let would_exceed_target = current_chars > 0 && combined_chars > config.chunk_target_chars;
        let would_exceed_max = current_chars > 0 && combined_chars > config.max_chunk_chars;

        if would_exceed_target || would_exceed_max {
            push_split_block(
                &mut chunks,
                section,
                &current_text,
                current_start,
                current_end,
            );
            let max_overlap_chars = config.max_chunk_chars.saturating_sub(block_chars);
            let max_overlap_chars = max_overlap_chars.saturating_sub(2);
            current_text = overlap_tail_chars(
                &current_text,
                config.chunk_overlap_chars.min(max_overlap_chars),
            );
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

fn split_oversized_block(block: Block, max_chunk_chars: usize) -> Vec<Block> {
    if max_chunk_chars == 0 || char_count(&block.text) <= max_chunk_chars {
        return vec![block];
    }

    let mut chunks = Vec::new();
    let mut current_text = String::new();
    let mut current_start = block.start_line;
    let mut current_end = block.start_line;

    for (offset, line) in block.text.lines().enumerate() {
        let line_no = block.start_line + offset;
        let line_chars = char_count(line);

        if line_chars > max_chunk_chars {
            push_oversized_block_piece(
                &mut chunks,
                &block,
                &current_text,
                current_start,
                current_end,
            );
            current_text.clear();

            for piece in split_long_line(line, max_chunk_chars) {
                push_oversized_block_piece(&mut chunks, &block, &piece, line_no, line_no);
            }
            current_start = line_no + 1;
            current_end = line_no + 1;
            continue;
        }

        let separator_chars = usize::from(!current_text.is_empty());
        if !current_text.is_empty()
            && char_count(&current_text) + separator_chars + line_chars > max_chunk_chars
        {
            push_oversized_block_piece(
                &mut chunks,
                &block,
                &current_text,
                current_start,
                current_end,
            );
            current_text.clear();
            current_start = line_no;
        }

        if !current_text.is_empty() {
            current_text.push('\n');
        } else {
            current_start = line_no;
        }
        current_text.push_str(line);
        current_end = line_no;
    }

    push_oversized_block_piece(
        &mut chunks,
        &block,
        &current_text,
        current_start,
        current_end,
    );
    chunks
}

fn split_long_line(line: &str, max_chunk_chars: usize) -> Vec<String> {
    if max_chunk_chars == 0 || char_count(line) <= max_chunk_chars {
        return vec![line.to_string()];
    }

    let mut pieces = Vec::new();
    let mut remaining = line.trim();

    while char_count(remaining) > max_chunk_chars {
        let split_at = preferred_split_byte(remaining, max_chunk_chars);
        let (left, right) = remaining.split_at(split_at);
        let left = left.trim();
        if !left.is_empty() {
            pieces.push(left.to_string());
        }
        remaining = right.trim_start();
    }

    let remaining = remaining.trim();
    if !remaining.is_empty() {
        pieces.push(remaining.to_string());
    }
    pieces
}

fn preferred_split_byte(value: &str, max_chunk_chars: usize) -> usize {
    let hard_cut = byte_index_after_chars(value, max_chunk_chars);
    let min_preferred_chars = max_chunk_chars.saturating_div(2);
    let mut last_whitespace = None;

    for (char_index, (byte_index, ch)) in value.char_indices().enumerate() {
        if byte_index >= hard_cut {
            break;
        }
        if ch.is_whitespace() && char_index >= min_preferred_chars {
            last_whitespace = Some(byte_index);
        }
    }

    last_whitespace.unwrap_or(hard_cut)
}

fn byte_index_after_chars(value: &str, chars: usize) -> usize {
    value
        .char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

fn push_oversized_block_piece(
    chunks: &mut Vec<Block>,
    source: &Block,
    text: &str,
    start_line: usize,
    end_line: usize,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    chunks.push(Block {
        heading_path: source.heading_path.clone(),
        heading_chain: source.heading_chain.clone(),
        heading_level: source.heading_level,
        text: text.to_string(),
        start_line,
        end_line: end_line.max(start_line),
        start_page: source.start_page,
        end_page: source.end_page,
    });
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
        start_page: section.start_page,
        end_page: section.end_page,
    });
}

fn paragraph_blocks(
    text: &str,
    start_line: usize,
    heading_path: &str,
    start_page: Option<usize>,
    end_page: Option<usize>,
) -> Vec<Block> {
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
                BlockLocation {
                    start_line: current_start,
                    end_line: line_no,
                    start_page,
                    end_page,
                },
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
        BlockLocation {
            start_line: current_start,
            end_line: final_line.max(start_line),
            start_page,
            end_page,
        },
        heading_path,
        &[],
        None,
    );
    blocks
}

fn flush_block(
    blocks: &mut Vec<Block>,
    current: &mut String,
    location: BlockLocation,
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
            start_line: location.start_line,
            end_line: location.end_line.max(location.start_line),
            start_page: location.start_page,
            end_page: location.end_page,
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
        document_kind: context.document_kind,
        title: context.title.to_string(),
        ordinal,
        heading_path: block.heading_path.clone(),
        heading_level: block.heading_level,
        text: trimmed.to_string(),
        start_line: block.start_line,
        end_line: block.end_line,
        start_page: block.start_page,
        end_page: block.end_page,
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
