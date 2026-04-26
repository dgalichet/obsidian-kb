mod common;

use assert_cmd::Command;

#[test]
fn init_is_idempotent() {
    let (temp, vault) = common::temp_vault();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .current_dir(temp.path())
        .args(["init", "--vault", vault.to_str().unwrap()])
        .assert()
        .success();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .current_dir(temp.path())
        .args(["init", "--vault", vault.to_str().unwrap()])
        .assert()
        .success();
    assert!(temp.path().join(".obsidian-kb.toml").exists());
    let config = std::fs::read_to_string(temp.path().join(".obsidian-kb.toml")).unwrap();
    assert!(config.contains("[vault]"));
    assert!(config.contains("[index]"));
    assert!(config.contains("[search]"));
    assert!(config.contains("[embeddings]"));
}

#[test]
fn index_uses_vault_config_from_current_directory() {
    let (temp, vault) = common::temp_vault();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .current_dir(temp.path())
        .args(["init", "--vault", vault.to_str().unwrap()])
        .assert()
        .success();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .current_dir(temp.path())
        .args(["index", "--changed-only", "--no-embeddings"])
        .assert()
        .success();
}
