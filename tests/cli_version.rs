use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_command_prints_package_version() {
    let expected_version = option_env!("OBSIDIAN_KB_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));

    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::eq(format!("obsidian-kb {expected_version}\n")));
}

#[test]
fn help_lists_mcp_command() {
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp"))
        .stdout(predicate::str::contains("serve"));
}
