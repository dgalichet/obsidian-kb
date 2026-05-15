use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_command_prints_package_version() {
    Command::cargo_bin("obsidian-kb")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::eq(format!(
            "obsidian-kb {}\n",
            env!("CARGO_PKG_VERSION")
        )));
}
