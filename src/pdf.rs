use anyhow::{Context, Result};
use lopdf::Document;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::chunking::{self, TextSection};
use crate::config::AppConfig;
use crate::models::{DocumentKind, Heading, ParsedNote};
use crate::vault;

struct PdfPage {
    number: usize,
    text: String,
}

struct PdfFileMetadata {
    absolute_path: PathBuf,
    relative_path: String,
    title: String,
    folder: String,
    hash: String,
    mtime: i64,
    size: u64,
}

pub fn parse_pdf_file(
    _vault_path: &Path,
    absolute_path: &Path,
    relative_path: &str,
    config: &AppConfig,
) -> Result<ParsedNote> {
    let metadata = std::fs::metadata(absolute_path)
        .with_context(|| format!("failed to read PDF metadata: {}", absolute_path.display()))?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or_default();
    let size = metadata.len();
    let title = pdf_title(relative_path);
    let folder = Path::new(relative_path)
        .parent()
        .map(vault::normalize_relative_path)
        .unwrap_or_default();

    if exceeds_pdf_size_limit(size, config) {
        return Ok(empty_pdf_note(
            PdfFileMetadata {
                absolute_path: absolute_path.to_path_buf(),
                relative_path: relative_path.to_string(),
                title,
                folder,
                hash: skipped_hash(size, mtime),
                mtime,
                size,
            },
            format!(
                "{relative_path}: skipped PDF larger than {} MiB",
                config.index.pdf.max_file_size_mb
            ),
        ));
    }

    let hash = file_hash(absolute_path)?;
    let pdf_file = PdfFileMetadata {
        absolute_path: absolute_path.to_path_buf(),
        relative_path: relative_path.to_string(),
        title,
        folder,
        hash,
        mtime,
        size,
    };
    let pages = match extract_pages(absolute_path) {
        Ok(pages) => pages,
        Err(error) => {
            return Ok(empty_pdf_note(
                pdf_file,
                format!("{relative_path}: failed to extract PDF text: {error}"),
            ));
        }
    };

    let mut warnings = Vec::new();
    if pages.is_empty() {
        warnings.push(format!("{relative_path}: PDF contains no extractable text"));
    }

    let body = pdf_body(&pages);
    let headings = pages
        .iter()
        .map(|page| Heading {
            level: 1,
            text: page_heading(page.number),
            line: page.number,
            slug: format!("page-{}", page.number),
        })
        .collect::<Vec<_>>();
    let sections = pages
        .iter()
        .map(|page| TextSection {
            heading_path: page_heading(page.number),
            heading_level: Some(1),
            text: page.text.clone(),
            start_line: page.number,
            end_line: page.number,
            start_page: Some(page.number),
            end_page: Some(page.number),
        })
        .collect::<Vec<_>>();
    let chunks = chunking::chunk_text_sections(
        relative_path,
        DocumentKind::Pdf,
        &pdf_file.title,
        &[],
        &sections,
        &config.index,
    );

    Ok(ParsedNote {
        path: pdf_file.relative_path,
        absolute_path: pdf_file.absolute_path,
        document_kind: DocumentKind::Pdf,
        title: pdf_file.title,
        folder: pdf_file.folder,
        hash: pdf_file.hash,
        mtime: pdf_file.mtime,
        size: pdf_file.size,
        frontmatter: Value::Null,
        body,
        aliases: Vec::new(),
        tags: Vec::new(),
        properties: Vec::new(),
        links: Vec::new(),
        headings,
        chunks,
        warnings,
    })
}

fn extract_pages(path: &Path) -> Result<Vec<PdfPage>> {
    let document =
        Document::load(path).with_context(|| format!("failed to load PDF: {}", path.display()))?;
    let mut pages = Vec::new();
    for page_number in document.get_pages().keys().copied() {
        let text = document.extract_text(&[page_number]).with_context(|| {
            format!(
                "failed to extract text from page {page_number} in {}",
                path.display()
            )
        })?;
        let text = normalize_pdf_text(&text);
        if !text.is_empty() {
            pages.push(PdfPage {
                number: page_number as usize,
                text,
            });
        }
    }
    Ok(pages)
}

fn empty_pdf_note(pdf_file: PdfFileMetadata, warning: String) -> ParsedNote {
    ParsedNote {
        path: pdf_file.relative_path,
        absolute_path: pdf_file.absolute_path,
        document_kind: DocumentKind::Pdf,
        title: pdf_file.title,
        folder: pdf_file.folder,
        hash: pdf_file.hash,
        mtime: pdf_file.mtime,
        size: pdf_file.size,
        frontmatter: Value::Null,
        body: String::new(),
        aliases: Vec::new(),
        tags: Vec::new(),
        properties: Vec::new(),
        links: Vec::new(),
        headings: Vec::new(),
        chunks: Vec::new(),
        warnings: vec![warning],
    }
}

fn exceeds_pdf_size_limit(size: u64, config: &AppConfig) -> bool {
    let limit = config.index.pdf.max_file_size_mb;
    limit > 0 && size > (limit as u64).saturating_mul(1024 * 1024)
}

fn file_hash(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read PDF: {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn skipped_hash(size: u64, mtime: i64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"skipped-pdf");
    hasher.update(&size.to_le_bytes());
    hasher.update(&mtime.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

fn pdf_title(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or(relative_path)
        .trim()
        .to_string()
}

fn page_heading(page: usize) -> String {
    format!("Page {page}")
}

fn pdf_body(pages: &[PdfPage]) -> String {
    pages
        .iter()
        .map(|page| format!("{}\n\n{}", page_heading(page.number), page.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn normalize_pdf_text(text: &str) -> String {
    let text = text.replace(['\r', '\u{0c}'], "\n");
    let mut output = String::new();
    let mut blank_lines = 0usize;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank_lines += 1;
            if blank_lines <= 1 && !output.ends_with('\n') {
                output.push('\n');
            }
            continue;
        }
        blank_lines = 0;
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(line);
    }
    output.trim().to_string()
}
