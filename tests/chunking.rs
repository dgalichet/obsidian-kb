use obsidian_kb::{chunking, config::IndexConfig, markdown};
use std::path::PathBuf;

#[test]
fn chunks_attach_heading_metadata() {
    let body = "# Top\n\nFirst paragraph with several words.\n\n## Details\n\nSecond paragraph with several more words.";
    let headings = markdown::extract_headings(body);
    let chunks = chunking::chunk_markdown(
        "note.md",
        "Note",
        &["tag".to_string()],
        body,
        &headings,
        &IndexConfig {
            store_dir: PathBuf::from(".obsidian-kb"),
            database_path: PathBuf::from(".obsidian-kb/metadata.sqlite"),
            tantivy_index_dir: PathBuf::from(".obsidian-kb/tantivy"),
            chunk_target_chars: 1000,
            chunk_overlap_chars: 0,
            max_chunk_chars: 1200,
            remove_diacritics: true,
        },
    );
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].heading_path.contains("Top"));
    assert_eq!(chunks[0].heading_level, Some(1));
    assert_eq!(chunks[1].heading_path, "Top > Details");
    assert_eq!(chunks[1].heading_level, Some(2));
    assert_eq!(chunks[0].note_path, "note.md");
}

#[test]
fn splits_long_heading_sections_by_paragraphs() {
    let paragraph = "This paragraph is intentionally long enough to contribute to chunk splitting while preserving paragraph boundaries and heading metadata.";
    let body = format!(
        "# Long\n\n{}\n\n{}\n\n{}\n\n```rust\nfn example() {{}}\n```\n\n{}",
        paragraph.repeat(12),
        paragraph.repeat(12),
        paragraph.repeat(12),
        paragraph.repeat(12)
    );
    let headings = markdown::extract_headings(&body);
    let chunks = chunking::chunk_markdown(
        "long.md",
        "Long",
        &[],
        &body,
        &headings,
        &IndexConfig {
            store_dir: PathBuf::from(".obsidian-kb"),
            database_path: PathBuf::from(".obsidian-kb/metadata.sqlite"),
            tantivy_index_dir: PathBuf::from(".obsidian-kb/tantivy"),
            chunk_target_chars: 1000,
            chunk_overlap_chars: 100,
            max_chunk_chars: 1800,
            remove_diacritics: true,
        },
    );

    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|chunk| chunk.heading_path == "Long"));
    assert!(chunks.iter().all(|chunk| chunk.heading_level == Some(1)));
    assert!(chunks.iter().any(|chunk| chunk.text.contains("```rust")));
}
