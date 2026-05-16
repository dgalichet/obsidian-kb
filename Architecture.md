# Architecture

This document explains how `obsidian-kb` works internally and why the main
technical choices were made. The README stays focused on end-user usage.

## Purpose

`obsidian-kb` means **Obsidian Knowledge Base**. It adds a local retrieval layer
on top of an Obsidian vault so a person or agent can retrieve a few relevant
Markdown chunks instead of reading the whole vault.

The project stays local-first:

- Markdown notes remain the source of truth.
- Indexes are generated locally.
- Embeddings are generated locally with FastEmbed.
- No hosted LLM API calls are made inside `obsidian-kb`.
- The tool does not write summaries or generated content back into the vault.

## Retrieval Pipeline

```text
Obsidian Markdown vault
  -> Markdown, frontmatter, tag, alias, and wikilink parsing
  -> heading-aware chunking
  -> SQLite metadata and embedding storage
  -> Tantivy BM25 full-text index
  -> local FastEmbed vector embeddings
  -> BM25 search, vector search, or hybrid search
  -> Reciprocal Rank Fusion
  -> optional shallow graph expansion through wikilinks/backlinks
  -> ranked chunks with citations and explainability fields
```

## Parsing And Chunking

The parser targets common Obsidian Markdown patterns:

- YAML frontmatter
- aliases
- tags
- wikilinks
- backlinks
- folders
- headings

Notes are split by Markdown heading sections. Long sections are split further
using the configured chunk target, overlap, and maximum size. This keeps results
small enough for agents while preserving heading context.

## Storage Layout

By default, `obsidian-kb init` creates `.obsidian-kb.toml`, and runtime indexes
live under `.obsidian-kb/`:

```text
.obsidian-kb.toml
.obsidian-kb/
  metadata.sqlite
  tantivy/
```

SQLite stores vault metadata, chunks, links, and embeddings. Tantivy stores the
BM25 full-text index.

Indexing is idempotent. Normal indexing compares content hash, mtime, and file
size. It reloads the vault, refreshes SQLite and Tantivy surfaces, and reuses
unchanged chunk embeddings when the chunk content hash still matches. Deleted
Markdown files disappear from SQLite and Tantivy on the next index run. The
legacy `--changed-only` flag is still accepted, but it is a deprecated alias for
normal indexing, not a true changed-only SQLite/Tantivy update.

`obsidian-kb index --rebuild` deletes the local SQLite database and Tantivy
directory, then rebuilds them from the vault. It does not modify Markdown notes.

## Search Modes

`bm25` uses Tantivy full-text search. It is best for exact names, commands,
errors, APIs, class names, acronyms, and file names. `obsidian-kb` searches
`title`, `heading_path`, `tags`, and `body`, with title and heading text weighted
higher.

`vector` uses local FastEmbed embeddings. It is best for vague conceptual
questions, synonyms, and ideas where the exact wording may differ from the notes.

`hybrid` combines BM25 and vector ranks with Reciprocal Rank Fusion. It is the
default because most real queries benefit from both lexical and semantic signals.

Search JSON includes explainability fields: final rank, final score, BM25 rank
and score, vector rank and score, graph boost, path, title, heading path, line
range, tags, snippet, and chunk ID.

## Graph Expansion

`--expand-graph` adds a shallow boost from directly linked notes and backlinks.
It starts from already relevant search results, then expands to nearby Obsidian
links within the configured depth and neighbor limits.

This is intentionally not full GraphRAG. `obsidian-kb` does not extract entities,
generate relationships, detect communities, summarize graph clusters, or perform
multi-step graph reasoning with an LLM.

## Why Rust

Rust gives the project a small native binary, fast filesystem traversal,
predictable memory use, and strong libraries for SQLite, Tantivy, parallel
scoring, and CLI ergonomics.

The goal is a dependable local command that an agent can call before deciding
which source chunks to read.

## Why Tantivy

Tantivy is a Rust full-text search engine with BM25 scoring. It keeps exact
search local, fast, and explainable, which matters for commands, API names,
errors, class names, acronyms, and file names.

## Why FastEmbed

FastEmbed runs embedding models locally. The default model is
`MultilingualE5Small`, which works well for mixed French and English vaults.

E5-style models are prefixed as:

- `passage: <chunk text>` for indexed chunks
- `query: <user query>` for searches

Model files are cached once in the user cache directory, such as
`~/Library/Caches/obsidian-kb/models` on macOS, and can be reused across vaults.

## Configuration Reference

`obsidian-kb init --vault /path/to/ObsidianVault` writes a config with this
default shape:

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

[benchmark]
enabled = false
log_path = ".obsidian-kb/benchmarks.jsonl"
include_query = false

[mcp]
idle_unload_seconds = 600
preload_embedder = false

[doctor.unresolved_links]
allow_forward_links = false
ignore_targets = []
ignore_globs = []
```

When `[benchmark]` is enabled, `obsidian-kb` appends one JSONL record per
command to `log_path`. Records include total elapsed time, command metadata, and
phase timings such as BM25 search, vector search, graph expansion, SQLite writes,
Tantivy rebuilds, and embedding rebuilds. Search queries are not written unless
`include_query = true`.

Vector search records `vector_ms` as the total vector phase and also breaks it
down into embedder initialization, query embedding, stored embedding loading, and
cosine scoring timings.

`obsidian-kb mcp` runs a local MCP stdio server exposing `search`, `show`,
`stats`, `warmup`, `unload`, and `status` tools. Vector and hybrid searches reuse
a warm local FastEmbed model while the process remains active. The cached model
is unloaded after `mcp.idle_unload_seconds` without stopping the MCP process; use
`0` to disable automatic unload. `preload_embedder = true` initializes the model
when the MCP server starts.

Config resolution order:

1. `--config /path/to/.obsidian-kb.toml`, when provided.
2. `--vault /path/to/vault`, which resolves
   `/path/to/vault/.obsidian-kb.toml`.
3. `.obsidian-kb.toml` in the current working directory.

## Command Reference

```bash
obsidian-kb init --vault /path/to/ObsidianVault

obsidian-kb index
obsidian-kb index --rebuild
obsidian-kb index --no-embeddings

obsidian-kb search "query"
obsidian-kb search "query" --mode bm25 --top 5
obsidian-kb search "query" --mode vector --top 5
obsidian-kb search "query" --mode hybrid --expand-graph --json
obsidian-kb search "query" --mode hybrid --include-text --max-chars 1200 --json

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

`doctor` checks the config, vault path, index directory, SQLite database, Tantivy
index, embedding model, exclude globs, Markdown file count, indexed file count,
chunks, and embeddings. JSON output includes structured issues, unresolved-link
categories, and target groups for automation.

## Agent Usage Rules

For vault questions, an agent should run `obsidian-kb search` before reading
files.

- Use `--mode hybrid --expand-graph` for conceptual questions.
- Use `--mode bm25` for exact names, commands, errors, APIs, classes, acronyms,
  or file names.
- Use `--mode vector` for vague conceptual questions.
- Read only the top relevant chunks returned by `show` or JSON `--include-text`.
- Cite paths and headings when summarizing.

## Current Limitations

- Vector search is brute force over SQLite-stored embeddings.
- Routine indexing preserves unchanged embeddings but still refreshes
  SQLite/Tantivy surfaces for consistency.
- Graph expansion is depth-limited and intentionally shallow.
- Markdown parsing targets common Obsidian patterns, not every Markdown
  extension.

## Roadmap

- For very large vaults, replace brute-force vector search with an ANN index such
  as HNSW, Qdrant, LanceDB, or another local vector index.
- Add more packaged distribution targets if non-macOS usage becomes necessary.

## Development

Useful checks:

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features
cargo fmt --all --check
```

Keep public functions typed and documented, prefer clear errors over silent
failure, keep indexing idempotent, and keep search explainable.

## Release

Releases are created from version tags. `Cargo.toml` stays on the local
development version `0.0.0-snapshot`; the release workflow derives the published
binary version from the pushed tag and injects it at build time.

```bash
git tag v1.2.3
git push origin v1.2.3
```

The release workflow builds the `aarch64-apple-darwin` archive, publishes it to
GitHub Releases, and updates the `dgalichet/homebrew-tap` formula when the
repository secret `HOMEBREW_TAP_TOKEN` is configured with write access to that
tap.

The tap repository should be public and initialized with at least one commit so
Homebrew can clone it as `dgalichet/tap`.
