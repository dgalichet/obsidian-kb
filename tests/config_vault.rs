use obsidian_kb::{
    config::{self, AppConfig},
    vault,
};
use std::path::{Path, PathBuf};

#[test]
fn config_defaults_and_loading_are_vault_relative() {
    let config =
        AppConfig::default_for_vault(Path::new("tests/fixtures/sample_vault"), None).unwrap();
    assert_eq!(config.search.default_mode, "hybrid");
    assert_eq!(config.embeddings.model, "MultilingualE5Small");
    assert!(config.index.exclude_headings.is_empty());
    assert!(config.index.properties.enabled);
    assert_eq!(config.index.properties.filter_keys, vec!["*".to_string()]);
    assert!(!config.benchmark.enabled);
    assert!(
        config
            .benchmark_log_path()
            .ends_with(".obsidian-kb/benchmarks.jsonl")
    );
    assert_eq!(config.mcp.idle_unload_seconds, 600);
    assert!(!config.mcp.preload_embedder);
    assert!(!config.doctor.unresolved_links.allow_forward_links);
    assert!(config.embedding_cache_dir().ends_with("obsidian-kb/models"));
    assert!(
        config
            .database_path()
            .ends_with(".obsidian-kb/metadata.sqlite")
    );
}

#[test]
fn vault_traversal_counts_markdown_and_honors_excludes() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(vault_path.join("Templates")).unwrap();
    std::fs::create_dir_all(vault_path.join("notes")).unwrap();
    std::fs::write(vault_path.join("notes/keep.md"), "# Keep").unwrap();
    std::fs::write(vault_path.join("Templates/skip.md"), "# Skip").unwrap();
    std::fs::write(vault_path.join("notes/ignore.txt"), "not markdown").unwrap();

    let mut config = AppConfig::default_for_vault(&vault_path, None).unwrap();
    config.config_dir = PathBuf::from(temp.path());
    let notes = vault::load_vault(&config).unwrap();

    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].path, "notes/keep.md");
    assert_eq!(vault::count_markdown_files(&config).unwrap(), 1);
}

#[test]
fn embedding_cache_dir_can_be_overridden_relative_to_config() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let mut config = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    config.embeddings.cache_dir = Some(PathBuf::from("shared-models"));

    assert_eq!(
        config.embedding_cache_dir(),
        temp.path().join("shared-models")
    );
}

#[test]
fn embedding_cache_dir_can_expand_home() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let mut config = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    config.embeddings.cache_dir = Some(PathBuf::from("~/obsidian-kb-models"));

    if let Some(home) = dirs::home_dir() {
        assert_eq!(
            config.embedding_cache_dir(),
            home.join("obsidian-kb-models")
        );
    }
}

#[test]
fn existing_configs_without_embedding_cache_dir_still_load() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let original = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    let path = temp.path().join(".obsidian-kb.toml");
    std::fs::write(&path, toml::to_string_pretty(&original).unwrap()).unwrap();

    let loaded = config::load_existing(None, Some(&path)).unwrap();

    assert!(loaded.embeddings.cache_dir.is_none());
    assert!(loaded.embedding_cache_dir().ends_with("obsidian-kb/models"));
}

#[test]
fn existing_configs_without_benchmark_section_still_load() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let original = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    let mut toml: toml::Value = toml::to_string_pretty(&original).unwrap().parse().unwrap();
    toml.as_table_mut().unwrap().remove("benchmark");

    let path = temp.path().join(".obsidian-kb.toml");
    std::fs::write(&path, toml::to_string_pretty(&toml).unwrap()).unwrap();

    let loaded = config::load_existing(None, Some(&path)).unwrap();

    assert!(!loaded.benchmark.enabled);
    assert!(
        loaded
            .benchmark_log_path()
            .ends_with(".obsidian-kb/benchmarks.jsonl")
    );
}

#[test]
fn existing_configs_without_mcp_section_still_load() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let original = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    let mut toml: toml::Value = toml::to_string_pretty(&original).unwrap().parse().unwrap();
    toml.as_table_mut().unwrap().remove("mcp");

    let path = temp.path().join(".obsidian-kb.toml");
    std::fs::write(&path, toml::to_string_pretty(&toml).unwrap()).unwrap();

    let loaded = config::load_existing(None, Some(&path)).unwrap();

    assert_eq!(loaded.mcp.idle_unload_seconds, 600);
    assert!(!loaded.mcp.preload_embedder);
}

#[test]
fn existing_configs_without_index_properties_still_load() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let original = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    let mut toml: toml::Value = toml::to_string_pretty(&original).unwrap().parse().unwrap();
    toml["index"].as_table_mut().unwrap().remove("properties");

    let path = temp.path().join(".obsidian-kb.toml");
    std::fs::write(&path, toml::to_string_pretty(&toml).unwrap()).unwrap();

    let loaded = config::load_existing(None, Some(&path)).unwrap();

    assert!(loaded.index.properties.enabled);
    assert_eq!(loaded.index.properties.filter_keys, vec!["*".to_string()]);
}

#[test]
fn existing_configs_without_index_exclude_headings_still_load() {
    let temp = tempfile::tempdir().unwrap();
    let vault_path = temp.path().join("vault");
    std::fs::create_dir_all(&vault_path).unwrap();

    let original = AppConfig::default_for_vault_in(&vault_path, None, temp.path()).unwrap();
    let mut toml: toml::Value = toml::to_string_pretty(&original).unwrap().parse().unwrap();
    toml["index"]
        .as_table_mut()
        .unwrap()
        .remove("exclude_headings");

    let path = temp.path().join(".obsidian-kb.toml");
    std::fs::write(&path, toml::to_string_pretty(&toml).unwrap()).unwrap();

    let loaded = config::load_existing(None, Some(&path)).unwrap();

    assert!(loaded.index.exclude_headings.is_empty());
}
