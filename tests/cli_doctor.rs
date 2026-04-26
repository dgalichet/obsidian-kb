mod common;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn doctor_reports_counts_without_requiring_hosted_services() {
    let (_temp, vault) = common::temp_vault();
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args([
            "index",
            "--vault",
            vault.to_str().unwrap(),
            "--no-embeddings",
        ])
        .assert()
        .success();

    let config_path = vault.join(".obsidian-kb.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace("enabled = true", "enabled = false"),
    )
    .unwrap();

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["doctor", "--vault", vault.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("markdown_files"))
        .stdout(predicate::str::contains("indexed_files"))
        .stdout(predicate::str::contains("embedding model"));
}
