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

#[allow(dead_code)]
pub fn write_minimal_pdf(path: &Path) {
    const STREAM_CRUFT: usize = 33;
    let content = "";
    let body = format!(
        "%PDF-1.5
1 0 obj<</Type/Pages/Kids[5 0 R]/Count 1/Resources 3 0 R/MediaBox[0 0 595 842]>>endobj
2 0 obj<</Type/Font/Subtype/Type1/BaseFont/Courier>>endobj
3 0 obj<</Font<</F1 2 0 R>>>>endobj
5 0 obj<</Type/Page/Parent 1 0 R/Contents[7 0 R 4 0 R]>>endobj
6 0 obj<</Type/Catalog/Pages 1 0 R>>endobj
7 0 obj<</Length 45>>stream
BT /F1 48 Tf 100 600 Td (Hello World!) Tj ET
endstream
endobj
4 0 obj<</Length {}>>stream
BT
/F1 48 Tf
100 600 Td
({}) Tj
ET
endstream endobj
",
        content.len() + STREAM_CRUFT,
        content
    );
    let pdf = format!(
        "{}xref
0 7
0000000000 65535 f 
0000000009 00000 n 
0000000096 00000 n 
0000000155 00000 n 
0000000387 00000 n 
0000000191 00000 n 
0000000254 00000 n 
0000000297 00000 n 
trailer
<</Root 6 0 R/Size 7>>
startxref
{}
%%EOF",
        body,
        body.len()
    );
    std::fs::write(path, pdf).expect("write minimal PDF");
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
