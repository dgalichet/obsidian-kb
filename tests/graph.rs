use obsidian_kb::{config::AppConfig, graph, vault};
use std::path::{Path, PathBuf};

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

#[test]
fn bare_links_do_not_resolve_to_pdf_titles() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();
    std::fs::write(
        vault_path.join("source.md"),
        "# Source\n\n[[hello-world]]\n\n![[hello-world.pdf]]",
    )
    .unwrap();
    std::fs::write(vault_path.join("hello-world.pdf"), "%PDF-1.4").unwrap();

    let mut config = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    config.index.pdf.enabled = true;
    config.config_dir = PathBuf::from(temp.path());
    let mut notes = vault::load_vault(&config).unwrap();
    graph::resolve_links(&mut notes);

    let source = notes.iter().find(|note| note.path == "source.md").unwrap();
    assert!(
        source
            .links
            .iter()
            .any(|link| link.target == "hello-world" && link.target_path.is_none())
    );
    assert!(
        source
            .links
            .iter()
            .any(|link| link.target == "hello-world.pdf"
                && link.target_path.as_deref() == Some("hello-world.pdf"))
    );
}
