use obsidian_kb::{
    chunking,
    config::{IndexConfig, PropertyIndexConfig},
    markdown,
};
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
        &index_config(),
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
            chunk_overlap_chars: 100,
            max_chunk_chars: 1800,
            ..index_config()
        },
    );

    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|chunk| chunk.heading_path == "Long"));
    assert!(chunks.iter().all(|chunk| chunk.heading_level == Some(1)));
    assert!(chunks.iter().any(|chunk| chunk.text.contains("```rust")));
}

#[test]
fn excludes_configured_heading_sections_and_descendants() {
    let body = "# Main\n\nSemantic content.\n\n## Relations\n\n[[linked-note]] should not influence retrieval.\n\n### Backlinks\n\nMore link noise.\n\n## Sources\n\nReference noise.\n\n## Details\n\nMore semantic content.";
    let headings = markdown::extract_headings(body);
    let chunks = chunking::chunk_markdown(
        "filtered.md",
        "Filtered",
        &[],
        body,
        &headings,
        &IndexConfig {
            exclude_headings: vec!["Relations".to_string(), "Sources".to_string()],
            ..index_config()
        },
    );

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].heading_path, "Main");
    assert_eq!(chunks[1].heading_path, "Main > Details");
    assert!(
        chunks
            .iter()
            .all(|chunk| !chunk.text.contains("linked-note"))
    );
    assert!(
        chunks
            .iter()
            .all(|chunk| !chunk.text.contains("Reference noise"))
    );
}

#[test]
fn excluding_headings_preserves_remaining_chunk_ids() {
    let body = "# Main\n\nSemantic content.\n\n## Relations\n\n[[linked-note]] should not influence retrieval.\n\n### Backlinks\n\nMore link noise.\n\n## Details\n\nMore semantic content.";
    let headings = markdown::extract_headings(body);
    let unfiltered = chunking::chunk_markdown(
        "filtered.md",
        "Filtered",
        &[],
        body,
        &headings,
        &index_config(),
    );
    let filtered = chunking::chunk_markdown(
        "filtered.md",
        "Filtered",
        &[],
        body,
        &headings,
        &IndexConfig {
            exclude_headings: vec!["Relations".to_string()],
            ..index_config()
        },
    );

    let unfiltered_details = unfiltered
        .iter()
        .find(|chunk| chunk.heading_path == "Main > Details")
        .unwrap();
    let filtered_details = filtered
        .iter()
        .find(|chunk| chunk.heading_path == "Main > Details")
        .unwrap();

    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered_details.ordinal, 1);
    assert_eq!(filtered_details.chunk_id, unfiltered_details.chunk_id);
}

fn index_config() -> IndexConfig {
    IndexConfig {
        store_dir: PathBuf::from(".obsidian-kb"),
        database_path: PathBuf::from(".obsidian-kb/metadata.sqlite"),
        tantivy_index_dir: PathBuf::from(".obsidian-kb/tantivy"),
        chunk_target_chars: 1000,
        chunk_overlap_chars: 0,
        max_chunk_chars: 1200,
        exclude_headings: Vec::new(),
        remove_diacritics: true,
        properties: PropertyIndexConfig::default(),
    }
}
