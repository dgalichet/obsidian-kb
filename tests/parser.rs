use obsidian_kb::{config::AppConfig, markdown, vault};
use std::path::Path;

#[test]
fn parses_frontmatter_tags_aliases_and_ignores_code_links() {
    let config =
        AppConfig::default_for_vault(Path::new("tests/fixtures/sample_vault"), None).unwrap();
    let content = std::fs::read_to_string("tests/fixtures/sample_vault/ai/obsidian.md").unwrap();
    let note = vault::parse_note_content(
        Path::new("tests/fixtures/sample_vault"),
        Path::new("tests/fixtures/sample_vault/ai/obsidian.md").to_path_buf(),
        "ai/obsidian.md",
        &content,
        0,
        content.len() as u64,
        &config,
    )
    .unwrap();

    assert!(note.aliases.contains(&"Obsidian".to_string()));
    assert!(note.tags.contains(&"real-tag".to_string()));
    assert!(!note.tags.contains(&"not-a-real-tag".to_string()));
    assert_eq!(note.links.len(), 1);
    assert_eq!(note.links[0].target, "rag");
}

#[test]
fn extracts_wikilink_anchor_and_display() {
    let links = markdown::extract_wikilinks("[[ai/rag#Reciprocal Rank Fusion|RRF]]");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, "ai/rag");
    assert_eq!(links[0].anchor.as_deref(), Some("Reciprocal Rank Fusion"));
    assert_eq!(links[0].display.as_deref(), Some("RRF"));
}

#[test]
fn extracts_singular_alias_string_tags_and_embedded_links() {
    let config =
        AppConfig::default_for_vault(Path::new("tests/fixtures/sample_vault"), None).unwrap();
    let content = std::fs::read_to_string("tests/fixtures/sample_vault/ai/contexte.md").unwrap();
    let note = vault::parse_note_content(
        Path::new("tests/fixtures/sample_vault"),
        Path::new("tests/fixtures/sample_vault/ai/contexte.md").to_path_buf(),
        "ai/contexte.md",
        &content,
        0,
        content.len() as u64,
        &config,
    )
    .unwrap();

    assert!(note.aliases.contains(&"Memoire de contexte".to_string()));
    assert!(note.aliases.contains(&"Contexte long".to_string()));
    assert!(note.tags.contains(&"ai/context".to_string()));
    assert!(note.tags.contains(&"retrieval".to_string()));

    let links = markdown::extract_wikilinks("![[attachments/architecture.png]]");
    assert_eq!(links.len(), 1);
    assert!(links[0].embedded);
    assert_eq!(links[0].raw, "![[attachments/architecture.png]]");
}

#[test]
fn extracts_wikilinks_with_escaped_pipes() {
    let links = markdown::extract_wikilinks(r"[[Notes/Foo\|Bar#Heading|Shown\|Alias]]");

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, "Notes/Foo|Bar");
    assert_eq!(links[0].anchor.as_deref(), Some("Heading"));
    assert_eq!(links[0].display.as_deref(), Some("Shown|Alias"));
}
