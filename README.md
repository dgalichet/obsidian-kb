<p align="center">
  <img src="assets/obsidian-kb-logo.png" alt="obsidian-kb logo" width="180">
</p>

# obsidian-kb

`obsidian-kb` is a local retrieval layer for Obsidian Markdown notes. It helps an LLM or coding agent retrieve relevant context without reading the whole vault.

`obsidian-kb` is not a magic second brain. It is not full GraphRAG, not a hosted AI tool, and not a system that writes summaries back into your vault.

## What It Does

- Parses Markdown notes, YAML frontmatter, aliases, tags, wikilinks, backlinks, folders, and headings.
- Chunks notes by Markdown heading sections, splitting only long sections.
- Indexes lexical search with Tantivy BM25.
- Generates local embeddings with FastEmbed.
- Stores vault metadata and embeddings in SQLite under `.obsidian-kb/`.
- Combines BM25 and vector ranks with Reciprocal Rank Fusion.
- Optionally expands search results to directly linked notes and backlinks.

## What It Does Not Do

- No OpenAI, Claude, ChatGPT, or hosted LLM API calls.
- No hosted vector database.
- No LangChain or LlamaIndex.
- No entity extraction, community detection, graph summaries, or LLM-generated graph.
- No automatic writing back into the Obsidian vault.

## Why Rust

Rust gives this tool a small native binary, fast filesystem traversal, predictable memory use, and good libraries for SQLite, Tantivy, parallel scoring, and CLI ergonomics. The goal is a local command Codex can run before deciding which files to read.

## Why Tantivy

Tantivy is a Rust full-text search engine with BM25 scoring. It is a good fit for exact names, commands, errors, APIs, class names, acronyms, and file names. `obsidian-kb` searches `title`, `heading_path`, `tags`, and `body`, with title and heading text weighted higher.

## Why FastEmbed

FastEmbed runs embedding models locally. The default model is `MultilingualE5Small`, which works well for mixed French and English vaults. E5-style models are prefixed as `passage: <chunk text>` for indexed chunks and `query: <user query>` for searches. Model files are cached once in the user cache directory, such as `~/Library/Caches/obsidian-kb/models` on macOS, and can be reused across vaults.

## GraphRAG Difference

Full GraphRAG usually builds entities, relationships, communities, summaries, and multi-step graph reasoning. `obsidian-kb` does not do that. It only uses Obsidian wikilinks and backlinks to add a small number of directly connected notes after lexical/vector retrieval has already found good seed results.

## Install

With Homebrew on macOS Apple Silicon:

```bash
brew install dgalichet/tap/obsidian-kb
```

Or download the matching macOS archive from the latest GitHub Release and put
`obsidian-kb` on your `PATH`.

For development from a Rust checkout:

```bash
cargo build --release
```

Then run the binary directly:

```bash
./target/release/obsidian-kb --help
```

## Configure

Create a local config:

```bash
obsidian-kb init --vault /path/to/ObsidianVault
```

This writes `.obsidian-kb.toml` and uses this default shape:

```toml
[vault]
path = "/path/to/ObsidianVault"
exclude_globs = [
  ".obsidian/**",
  ".obsidian-kb/**",
  ".trash/**",
  "Templates/**",
  "**/*.excalidraw.md"
]

[index]
store_dir = ".obsidian-kb"
database_path = ".obsidian-kb/metadata.sqlite"
tantivy_index_dir = ".obsidian-kb/tantivy"
chunk_target_chars = 3000
chunk_overlap_chars = 300
max_chunk_chars = 5000
remove_diacritics = true

[search]
default_mode = "hybrid"
bm25_candidates = 80
vector_candidates = 80
final_top_k = 10
rrf_k = 60
bm25_weight = 1.0
vector_weight = 1.0
graph_weight = 0.25
graph_depth = 1
graph_max_neighbors = 20

[embeddings]
enabled = true
provider = "fastembed"
model = "MultilingualE5Small"
batch_size = 64
normalize = true
# Optional override. Defaults to the user cache directory.
# cache_dir = "~/Library/Caches/obsidian-kb/models"

[doctor.unresolved_links]
allow_forward_links = false
ignore_targets = []
ignore_globs = []
```

## Index

```bash
obsidian-kb index
obsidian-kb index --rebuild
obsidian-kb index --changed-only
```

Normal indexing is idempotent and compares content hash, mtime, and file size. Unchanged chunk embeddings are reused when the chunk content hash still matches. Deleted Markdown files disappear from SQLite and Tantivy on the next index run.

`--rebuild` deletes the local SQLite database and Tantivy directory, then rebuilds from the vault. The tool does not modify Markdown notes.

## Search

```bash
obsidian-kb search "Obsidian RAG second brain"
obsidian-kb search "Service Connect TLS App Mesh" --mode bm25 --top 5
obsidian-kb search "Comment eviter la saturation du contexte ?" --mode vector --top 5
obsidian-kb search "query" --mode hybrid --expand-graph --json
obsidian-kb search "query" --mode hybrid --top 5 --include-text --max-chars 1200 --json
```

Search JSON includes explainability fields: final rank, final score, BM25 rank and score, vector rank and score, graph boost, path, title, heading path, line range, tags, snippet, and chunk ID.
Use `--include-text` with JSON when an agent needs compact source context without a separate `show` call. `--max-chars 0` includes the full chunk text.

## Inspect

```bash
obsidian-kb show <chunk-id>
obsidian-kb show <chunk-id> --json

obsidian-kb graph "Obsidian" --depth 1
obsidian-kb graph "Obsidian" --depth 1 --json

obsidian-kb stats
obsidian-kb stats --json

obsidian-kb doctor
obsidian-kb doctor --json

obsidian-kb version
```

`doctor` checks the config, vault path, index directory, SQLite database, Tantivy index, embedding model, exclude globs, Markdown file count, indexed file count, chunks, and embeddings.
JSON output includes structured issues, unresolved-link categories, and target groups for automation.

## Codex Usage

For vault questions, Codex should run `obsidian-kb search` before reading files.

- Use `--mode hybrid --expand-graph` for conceptual questions.
- Use `--mode bm25` for exact names, commands, errors, APIs, classes, acronyms, or file names.
- Use `--mode vector` for vague conceptual questions.
- Read only the top relevant chunks returned by `show`.
- Cite paths and headings when summarizing.

## Privacy

All indexes are local. Metadata and embeddings are stored in SQLite. BM25 data is stored in Tantivy. FastEmbed may download a local embedding model the first time and caches it outside the vault by default, but `obsidian-kb` itself does not call hosted LLM APIs.

## Current Limitations

- Vector search is brute force over SQLite-stored embeddings.
- Incremental indexing preserves unchanged embeddings but still refreshes SQLite/Tantivy surfaces for consistency.
- Graph expansion is depth-limited and intentionally shallow.
- Markdown parsing targets common Obsidian patterns, not every Markdown extension.

## Roadmap

- For very large vaults, replace brute-force vector search with an ANN index such as HNSW, Qdrant, LanceDB, or another local vector index.
- Add more packaged distribution targets if non-macOS usage becomes necessary.

## Release

Releases are created from version tags. Update `Cargo.toml`, commit the change,
then create and push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds the `aarch64-apple-darwin` archive, publishes it to
GitHub Releases, and updates the `dgalichet/homebrew-tap` formula when the
repository secret `HOMEBREW_TAP_TOKEN` is configured with write access to that
tap.

The tap repository should be public and initialized with at least one commit so
Homebrew can clone it as `dgalichet/tap`.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
