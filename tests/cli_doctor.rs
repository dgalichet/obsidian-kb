mod common;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

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

#[test]
fn doctor_json_reports_structured_unresolved_links() {
    let (_temp, vault) = common::temp_vault();
    std::fs::write(
        vault.join("edge/assets.md"),
        "# Asset Embed\n\n![[attachments/architecture.png]]\n",
    )
    .unwrap();

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

    let output = Command::cargo_bin("obsidian-kb")
        .unwrap()
        .args(["doctor", "--vault", vault.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert!(report["warning_count"].as_u64().unwrap() > 0);
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue["category"] == "possible_forward_link"
            && issue["file"] == "edge/broken-links.md"
            && issue["target"] == "Missing Note"
            && issue["link_type"] == "wikilink"
    }));
    assert!(report["issues"].as_array().unwrap().iter().any(|issue| {
        issue["level"] == "info"
            && issue["category"] == "asset_embed"
            && issue["file"] == "edge/assets.md"
            && issue["target"] == "attachments/architecture.png"
    }));
    assert!(
        report["unresolved_link_groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["target"] == "Missing Note"
                && group["category"] == "possible_forward_link"
                && group["occurrences"] == 1)
    );
}
