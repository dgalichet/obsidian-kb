use blake3::Hasher;

use crate::config::IndexConfig;
use crate::markdown;
use crate::models::{ChunkRecord, Heading};

#[derive(Debug, Clone)]
struct Block {
    heading_path: String,
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
    let sections = sections_by_heading(body, headings);
    let context = ChunkContext {
        note_path,
        title,
        tags,
    };
    let mut chunks = Vec::new();

    for section in sections {
        if char_count(&section.text) <= config.max_chunk_chars {
            push_chunk(
                &mut chunks,
                &context,
                &section.heading_path,
                section.heading_level,
                &section.text,
                section.start_line,
                section.end_line,
            );
        } else {
            split_long_section(&mut chunks, &context, &section, config);
        }
    }

    chunks
}

fn sections_by_heading(body: &str, headings: &[Heading]) -> Vec<Block> {
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut heading_index = 0;
    let mut current = String::new();
    let mut current_start = 1;
    let mut current_heading = String::new();
    let mut current_level = None;

    for (line_index, line) in body.lines().enumerate() {
        let line_no = line_index + 1;
        while heading_index < headings.len() && headings[heading_index].line == line_no {
            flush_block(
                &mut sections,
                &mut current,
                current_start,
                line_no.saturating_sub(1),
                &current_heading,
                current_level,
            );
            let heading = &headings[heading_index];
            heading_stack.retain(|(level, _)| *level < heading.level);
            heading_stack.push((heading.level, heading.text.clone()));
            current_heading = heading_stack
                .iter()
                .map(|(_, text)| text.as_str())
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

    let final_line = body.lines().count().max(1);
    flush_block(
        &mut sections,
        &mut current,
        current_start,
        final_line,
        &current_heading,
        current_level,
    );
    sections
}

fn split_long_section(
    chunks: &mut Vec<ChunkRecord>,
    context: &ChunkContext<'_>,
    section: &Block,
    config: &IndexConfig,
) {
    let blocks = paragraph_blocks(&section.text, section.start_line, &section.heading_path);
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
            push_chunk(
                chunks,
                context,
                &section.heading_path,
                section.heading_level,
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

    push_chunk(
        chunks,
        context,
        &section.heading_path,
        section.heading_level,
        &current_text,
        current_start,
        current_end,
    );
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
    heading_level: Option<usize>,
) {
    let text = current.trim();
    if !text.is_empty() {
        blocks.push(Block {
            heading_path: heading_path.to_string(),
            heading_level,
            text: text.to_string(),
            start_line,
            end_line: end_line.max(start_line),
        });
    }
    current.clear();
}

fn push_chunk(
    chunks: &mut Vec<ChunkRecord>,
    context: &ChunkContext<'_>,
    heading_path: &str,
    heading_level: Option<usize>,
    text: &str,
    start_line: usize,
    end_line: usize,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let ordinal = chunks.len();
    let text_hash = blake3::hash(trimmed.as_bytes()).to_hex().to_string();
    let mut hasher = Hasher::new();
    hasher.update(context.note_path.as_bytes());
    hasher.update(heading_path.as_bytes());
    hasher.update(&ordinal.to_le_bytes());
    hasher.update(text_hash.as_bytes());
    let chunk_id = hasher.finalize().to_hex().to_string();
    chunks.push(ChunkRecord {
        file_id: 0,
        chunk_id,
        note_path: context.note_path.to_string(),
        title: context.title.to_string(),
        ordinal,
        heading_path: heading_path.to_string(),
        heading_level,
        text: trimmed.to_string(),
        start_line,
        end_line,
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
