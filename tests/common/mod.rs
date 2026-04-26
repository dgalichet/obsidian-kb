use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn fixture_vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_vault")
}

pub fn temp_vault() -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let source = fixture_vault();
    let target = temp.path().join("vault");
    copy_dir(&source, &target);
    (temp, target)
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("create target dir");
    for entry in std::fs::read_dir(source).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).expect("copy fixture file");
        }
    }
}
