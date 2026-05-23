use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Link topology for generated synthetic vault notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyntheticLinkMode {
    None,
    Sparse,
}

impl SyntheticLinkMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "sparse" => Ok(Self::Sparse),
            _ => bail!("unknown synthetic link mode `{value}`; use `none` or `sparse`"),
        }
    }
}

/// Options for deterministic synthetic benchmark vault generation.
#[derive(Debug, Clone)]
pub struct SyntheticVaultOptions {
    pub chunks: usize,
    pub chunks_per_note: usize,
    pub link_mode: SyntheticLinkMode,
    pub overwrite: bool,
}

impl Default for SyntheticVaultOptions {
    fn default() -> Self {
        Self {
            chunks: 1_000,
            chunks_per_note: 100,
            link_mode: SyntheticLinkMode::Sparse,
            overwrite: false,
        }
    }
}

/// Summary of generated benchmark vault content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntheticVaultReport {
    pub chunks: usize,
    pub notes: usize,
    pub chunks_per_note: usize,
    pub link_mode: String,
}

/// Generates a deterministic synthetic Obsidian vault for benchmark runs.
pub fn generate_synthetic_vault(
    output: &Path,
    options: &SyntheticVaultOptions,
) -> Result<SyntheticVaultReport> {
    if options.chunks == 0 {
        bail!("--chunks must be greater than 0");
    }
    if options.chunks_per_note == 0 {
        bail!("--chunks-per-note must be greater than 0");
    }
    if output.exists() {
        if options.overwrite {
            fs::remove_dir_all(output)
                .with_context(|| format!("failed to remove {}", output.display()))?;
        } else {
            bail!(
                "output path already exists: {}; pass --overwrite to replace it",
                output.display()
            );
        }
    }

    let notes = options.chunks.div_ceil(options.chunks_per_note);
    let notes_dir = output.join("notes");
    fs::create_dir_all(&notes_dir)
        .with_context(|| format!("failed to create {}", notes_dir.display()))?;

    let mut chunk = 0usize;
    for note_index in 1..=notes {
        let folder = notes_dir.join(format!("{:03}", note_index / 100));
        fs::create_dir_all(&folder)
            .with_context(|| format!("failed to create {}", folder.display()))?;
        let note_path = folder.join(format!("synthetic-{note_index:05}.md"));
        let mut file = fs::File::create(&note_path)
            .with_context(|| format!("failed to create {}", note_path.display()))?;

        write_frontmatter(&mut file, note_index)?;

        for _ in 0..options.chunks_per_note {
            if chunk >= options.chunks {
                break;
            }
            chunk += 1;
            write_chunk(&mut file, chunk)?;
        }
        write_sparse_links(&mut file, note_index, notes, options.link_mode)?;
    }

    Ok(SyntheticVaultReport {
        chunks: options.chunks,
        notes,
        chunks_per_note: options.chunks_per_note,
        link_mode: match options.link_mode {
            SyntheticLinkMode::None => "none".to_string(),
            SyntheticLinkMode::Sparse => "sparse".to_string(),
        },
    })
}

/// Returns a deterministic normalized vector for synthetic benchmarks.
pub fn deterministic_unit_vector(seed: &str, dim: usize) -> Vec<f32> {
    let hash = blake3::hash(seed.as_bytes());
    let mut state = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().unwrap());
    let mut vector = Vec::with_capacity(dim);
    let mut norm = 0.0f32;

    for _ in 0..dim {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let unit = ((state >> 40) as f32) / ((1u32 << 24) as f32);
        let value = unit.mul_add(2.0, -1.0);
        norm += value * value;
        vector.push(value);
    }

    let norm = norm.sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn write_frontmatter(mut file: impl Write, note_index: usize) -> Result<()> {
    writeln!(file, "---")?;
    writeln!(file, "tags: [synthetic, benchmark]")?;
    writeln!(file, "status: generated")?;
    writeln!(file, "synthetic_group: {}", note_index / 100)?;
    writeln!(file, "---")?;
    writeln!(file)?;
    Ok(())
}

fn write_sparse_links(
    mut file: impl Write,
    note_index: usize,
    notes: usize,
    mode: SyntheticLinkMode,
) -> Result<()> {
    if mode == SyntheticLinkMode::None {
        return Ok(());
    }

    writeln!(file, "<!-- Synthetic graph anchors -->")?;
    if note_index > 1 {
        writeln!(
            file,
            "[[synthetic-{previous:05}]]",
            previous = note_index - 1
        )?;
    }
    if note_index < notes {
        writeln!(file, "[[synthetic-{next:05}]]", next = note_index + 1)?;
    }
    if note_index % 10 == 0 {
        let hub = note_index - note_index % 10 + 1;
        writeln!(file, "[[synthetic-{hub:05}]]")?;
    }
    writeln!(file)?;
    Ok(())
}

fn write_chunk(mut file: impl Write, chunk: usize) -> Result<()> {
    let theme = match chunk % 5 {
        0 => "agent memory and local knowledge base retrieval",
        1 => "BM25 lexical matching and explainable ranking",
        2 => "vector search, cosine scoring, and warm cache behavior",
        3 => "SQLite metadata, Tantivy indexing, and graph expansion",
        _ => "MCP HTTP transport and Obsidian vault search",
    };

    writeln!(file, "## Synthetic chunk {chunk:05}")?;
    writeln!(file)?;
    writeln!(
        file,
        "This benchmark chunk number {chunk:05} discusses {theme}. It repeats \
         local-first retrieval, graph expansion, BM25 lexical matching, vector \
         search, SQLite metadata, Tantivy indexing, MCP HTTP cache behavior, \
         and Obsidian vault search."
    )?;
    writeln!(file)?;
    writeln!(
        file,
        "Related topic anchors: agent memory, local knowledge base, synthetic \
         public benchmark, deterministic corpus, chunk hash {chunk:05}."
    )?;
    writeln!(file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_exact_chunk_count_without_extra_intro_chunk() {
        let temp = tempfile::tempdir().unwrap();
        let report = generate_synthetic_vault(
            temp.path(),
            &SyntheticVaultOptions {
                chunks: 250,
                chunks_per_note: 100,
                link_mode: SyntheticLinkMode::Sparse,
                overwrite: true,
            },
        )
        .unwrap();

        assert_eq!(report.chunks, 250);
        assert_eq!(report.notes, 3);
        let headings = std::fs::read_dir(temp.path().join("notes"))
            .unwrap()
            .flat_map(|entry| {
                std::fs::read_dir(entry.unwrap().path())
                    .unwrap()
                    .map(|file| std::fs::read_to_string(file.unwrap().path()).unwrap())
            })
            .flat_map(|content| {
                content
                    .lines()
                    .filter(|line| line.starts_with("## Synthetic chunk "))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .count();
        assert_eq!(headings, 250);
    }

    #[test]
    fn deterministic_vectors_are_normalized_and_stable() {
        let left = deterministic_unit_vector("chunk-a", 384);
        let right = deterministic_unit_vector("chunk-a", 384);
        assert_eq!(left, right);

        let norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }
}
