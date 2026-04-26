use obsidian_kb::{config::AppConfig, graph, vault};
use std::path::Path;

#[test]
fn resolves_links_and_reports_duplicate_aliases() {
    let config =
        AppConfig::default_for_vault(Path::new("tests/fixtures/sample_vault"), None).unwrap();
    let mut notes = vault::load_vault(&config).unwrap();
    let report = graph::resolve_links(&mut notes);
    let rag = notes.iter().find(|note| note.path == "ai/rag.md").unwrap();
    assert!(
        rag.links
            .iter()
            .any(|link| link.target_path.as_deref() == Some("ai/obsidian.md"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("duplicate alias `rag`"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("unresolved link `[[Missing Note]]`"))
    );
    let contexte = notes
        .iter()
        .find(|note| note.path == "ai/contexte.md")
        .unwrap();
    assert!(
        contexte
            .links
            .iter()
            .any(|link| link.target_path.as_deref() == Some("ai/lost-in-the-middle.md"))
    );
}
