use obsidian_kb::{
    chunking,
    config::{IndexConfig, PdfIndexConfig, PropertyIndexConfig},
    markdown,
    models::DocumentKind,
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
fn splits_single_paragraph_that_exceeds_max_chunk_chars() {
    let paragraph = (0..80)
        .map(|index| format!("word{index:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    let body = format!("# Long\n\n{paragraph}");
    let headings = markdown::extract_headings(&body);
    let chunks = chunking::chunk_markdown(
        "long-paragraph.md",
        "Long Paragraph",
        &[],
        &body,
        &headings,
        &IndexConfig {
            chunk_target_chars: 160,
            chunk_overlap_chars: 25,
            max_chunk_chars: 180,
            ..index_config()
        },
    );

    assert!(chunks.len() > 1);
    assert_chunks_within_max(&chunks, 180);
    assert!(chunks.iter().all(|chunk| chunk.heading_path == "Long"));
    assert!(chunks.iter().any(|chunk| chunk.text.contains("word00")));
    assert!(chunks.iter().any(|chunk| chunk.text.contains("word79")));
}

#[test]
fn splits_unbroken_long_token_that_exceeds_max_chunk_chars() {
    let token = "x".repeat(350);
    let chunks = chunking::chunk_markdown(
        "token.md",
        "Token",
        &[],
        &token,
        &[],
        &IndexConfig {
            chunk_target_chars: 64,
            max_chunk_chars: 64,
            ..index_config()
        },
    );

    assert!(chunks.len() > 1);
    assert_chunks_within_max(&chunks, 64);
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<Vec<_>>()
            .join(""),
        token
    );
}

#[test]
fn splits_long_text_section_and_preserves_page_metadata() {
    let text = (0..90)
        .map(|index| format!("pageword{index:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    let sections = vec![chunking::TextSection {
        heading_path: "Page 7".to_string(),
        heading_level: Some(1),
        text,
        start_line: 7,
        end_line: 7,
        start_page: Some(7),
        end_page: Some(7),
    }];
    let chunks = chunking::chunk_text_sections(
        "paper.pdf",
        DocumentKind::Pdf,
        "Paper",
        &[],
        &sections,
        &IndexConfig {
            chunk_target_chars: 140,
            max_chunk_chars: 150,
            ..index_config()
        },
    );

    assert!(chunks.len() > 1);
    assert_chunks_within_max(&chunks, 150);
    assert!(chunks.iter().all(|chunk| {
        chunk.document_kind == DocumentKind::Pdf
            && chunk.heading_path == "Page 7"
            && chunk.heading_level == Some(1)
            && chunk.start_page == Some(7)
            && chunk.end_page == Some(7)
    }));
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

fn assert_chunks_within_max(chunks: &[obsidian_kb::models::ChunkRecord], max_chars: usize) {
    for chunk in chunks {
        let chars = chunk.text.chars().count();
        assert!(
            chars <= max_chars,
            "chunk has {chars} chars, expected at most {max_chars}: {}",
            chunk.text
        );
    }
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
        pdf: PdfIndexConfig::default(),
    }
}
