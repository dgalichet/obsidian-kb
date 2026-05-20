<p align="center">
  <img src="assets/obsidian-kb-logo.png" alt="obsidian-kb logo" width="180">
</p>

# obsidian-kb

**Search your Obsidian vault like a knowledge base. Keep it local.**

`obsidian-kb` means **Obsidian Knowledge Base**. It turns an Obsidian vault into
a local knowledge base that you, a script, or a coding agent can search without
opening the whole vault.

It is built for the moment where keyword search is too narrow, semantic search is
too fuzzy, and sending an entire vault to an LLM is not acceptable. Your Markdown
notes stay the source of truth; `obsidian-kb` adds a local retrieval layer around
them.

## Why Use It

- Find the right note section, not just the right file.
- Combine exact keyword search with semantic search.
- Follow useful Obsidian wikilinks and backlinks when context is connected.
- Get traceable results with file paths, headings, line ranges, scores, and
  snippets.
- Let LLM agents read only the few chunks they need.
- Keep indexing, embeddings, and metadata local.

`obsidian-kb` does not call OpenAI, Claude, ChatGPT, or any hosted LLM API. It
does not write summaries into your vault, and it does not replace Obsidian. It is
a fast local companion for retrieval.

## Install

With Homebrew on macOS Apple Silicon:

```bash
brew install dgalichet/tap/obsidian-kb
```

Or download the matching macOS archive from the
[latest GitHub Release](https://github.com/dgalichet/obsidian-kb/releases/latest)
and put `obsidian-kb` on your `PATH`.

For development from a Rust checkout:

```bash
cargo build --release
./target/release/obsidian-kb --help
```

## Quick Start

Create a local config for your vault:

```bash
obsidian-kb init --vault /path/to/ObsidianVault
```

Build the local index:

```bash
obsidian-kb index
```

Ask a question:

```bash
obsidian-kb search "How do I avoid overloading an agent context?"
```

Search for an exact name, command, API, or error:

```bash
obsidian-kb search "Service Connect TLS App Mesh" --mode bm25 --top 5
```

Search for a vague idea:

```bash
obsidian-kb search "notes about context saturation" --mode vector --top 5
```

Find notes similar to an indexed note:

```bash
obsidian-kb related "ai/contexte.md" --top 5
```

Find notes similar to draft text that is not indexed yet:

```bash
pbpaste | obsidian-kb related --stdin --top 5
obsidian-kb related --text "draft paragraph..." --top 5
```

Use hybrid search plus the Obsidian graph:

```bash
obsidian-kb search "local RAG with Obsidian" --mode hybrid --expand-graph
```

Filter by indexed tags and frontmatter properties:

```bash
obsidian-kb search "hybrid retrieval" --tag retrieval/hybrid --property status=active
obsidian-kb search --tag business --json
```

## Search Modes

| Mode | Best for | Example |
| --- | --- | --- |
| `bm25` | Exact names, commands, errors, APIs, acronyms, file names | `cargo clippy warning` |
| `vector` | Vague concepts, synonyms, half-remembered ideas | `how to reduce context noise` |
| `hybrid` | Everyday search where exact and semantic signals both matter | `Obsidian RAG second brain` |

Add `--expand-graph` when linked notes and backlinks are likely to add useful
context:

```bash
obsidian-kb search "Obsidian RAG second brain" --mode hybrid --expand-graph
```

For automation, use JSON:

```bash
obsidian-kb search "query" --mode hybrid --expand-graph --json
obsidian-kb search "query" --mode hybrid --top 5 --include-text --max-chars 1200 --json
```

The JSON output is grouped by note. Each note includes its best chunk, matched
chunk count, matched chunks with headings, line ranges, snippets, chunk IDs,
ranks, scores, and graph boosts.

## Related Notes

`related` searches for notes that are semantically close to a source note or to
draft text. For indexed notes, it reuses the stored chunk embeddings for that
note, averages them into a source vector, scores indexed chunks, excludes the
source note, and aggregates the best matching chunks by note.

```bash
obsidian-kb related "note title or path" --top 10 --json
obsidian-kb related --stdin --top 10 --json
```

Use `--candidates N` to change how many vector chunk candidates are scored
before note-level aggregation. With benchmarking enabled, `related` logs the
same vector load/scoring phases as `search`, plus `related_aggregate_notes_ms`.
In MCP and HTTP serve mode, repeated vector and related searches reuse cached
stored embeddings and report `vector_embeddings_cached`.

## Index Exclusions

Use `index.exclude_headings` to keep noisy metadata sections out of BM25,
embeddings, and related-note scoring:

```toml
[index]
exclude_headings = ["Relations", "Sources"]
```

Heading matches are case-insensitive and include descendants, so excluding
`Relations` also excludes `### Backlinks` under `## Relations`. Full heading
paths are supported for narrower exclusions, for example
`"Project A > Relations"`. Wikilinks are still extracted from the whole note
for graph context.

## Structured Filters

`obsidian-kb` indexes tags and simple YAML frontmatter properties as structured
metadata. Use `--tag` to require a tag and `--property KEY=VALUE` to require a
frontmatter property value. Repeat either option to combine filters with AND
semantics:

```bash
obsidian-kb search "agents" --tag ai/context --property status=active
obsidian-kb search --property type=book --property status=reading --json
obsidian-kb search --property 'created>=2026-01-01' --json
```

Filter-only searches are allowed when at least one `--tag` or `--property`
filter is present. Tag filters include tags found in frontmatter and Markdown
body text. Property filters use simple scalar values and scalar arrays from
frontmatter. Supported property operators are `=`, `!=`, `>`, `>=`, `<`, and
`<=`. Quote filters containing `<` or `>` in shells.

Discover available filters before guessing:

```bash
obsidian-kb tags
obsidian-kb tags --prefix ai --json
obsidian-kb properties
obsidian-kb properties --key status --json
```

Frontmatter property indexing is configurable in `.obsidian-kb.toml`:

```toml
[index.properties]
enabled = true
filter_keys = ["*"]
ignored_keys = ["cssclasses", "template", "id", "uuid", "publish", "dg-*"]
max_value_chars = 200
```

By default all simple properties are available for exact filters except noisy
or technical keys. Property filters do not change BM25 or vector scoring yet;
they narrow the result set after candidate retrieval.

## Daily Workflow

Re-index after changing notes:

```bash
obsidian-kb index
```

Routine indexing reloads the vault, refreshes SQLite metadata and Tantivy, and
reuses embeddings for chunks whose content hash has not changed. The legacy
`--changed-only` flag is still accepted, but it is not a true changed-only
SQLite/Tantivy indexer.

Rebuild everything if you want a clean index:

```bash
obsidian-kb index --rebuild
```

Inspect a result:

```bash
obsidian-kb show <chunk-id>
obsidian-kb show <chunk-id> --json
```

Check that the vault and indexes are healthy:

```bash
obsidian-kb doctor
obsidian-kb stats
```

Explore direct links around a note:

```bash
obsidian-kb graph "Obsidian" --depth 1
```

## Using With LLM Agents

Give the agent a simple rule: search first, read second.

Recommended defaults:

- Use `obsidian-kb search --mode hybrid --expand-graph` for conceptual
  questions.
- Use `--mode bm25` for exact names, commands, errors, APIs, classes, acronyms,
  and file names.
- Use `--mode vector` for vague conceptual questions.
- Read only the top relevant chunks.
- Cite file paths and headings when summarizing.

This keeps answers grounded while avoiding bulk reads of the whole vault.

For a complete agent and MCP usage guide, see
[USING_OBSIDIAN_KB_WITH_AGENTS.md](USING_OBSIDIAN_KB_WITH_AGENTS.md).

## Local Files

`obsidian-kb init` writes a `.obsidian-kb.toml` config file. Index data is stored
under `.obsidian-kb/` by default:

- SQLite metadata and embeddings
- Tantivy full-text index
- chunk and link metadata

The tool does not modify your Markdown notes during indexing or search.

FastEmbed may download a local embedding model the first time embeddings are
built. Model files are cached in the user cache directory by default, such as
`~/Library/Caches/obsidian-kb/models` on macOS, and can be reused across vaults.

Optional benchmark logging can be enabled in `.obsidian-kb.toml` with
`[benchmark] enabled = true`. Timings are appended locally as JSONL under
`.obsidian-kb/benchmarks.jsonl` by default, and search query text is omitted
unless `include_query = true`.

For MCP clients, `obsidian-kb mcp` runs a local stdio server with search,
related, show, graph, tags, properties, stats, warmup, unload, and status tools.
The MCP `search` tool accepts `tags` and `properties` arrays, plus `tag` and
`property` aliases for single filters. Hybrid and vector searches keep the local
embedding model and stored embeddings warm between requests, then unload them
automatically after the configured idle timeout.

For local HTTP clients, `obsidian-kb serve` binds to `127.0.0.1:27124` by
default and prints the chosen URL. Use `--port` to override the port. It exposes
`GET /health`, `GET /status`, `POST /search`, `POST /show`, `POST /graph`,
`POST /index/refresh`, `POST /shutdown`, and `POST /mcp`. The `/mcp` endpoint
accepts MCP JSON-RPC over streamable HTTP and can return either JSON or
`text/event-stream` responses.

## What It Is Not

`obsidian-kb` is intentionally small in scope:

- not a hosted AI product;
- not a hosted vector database;
- not a LangChain or LlamaIndex wrapper;
- not a full GraphRAG system;
- not an automatic note writer.

For implementation details, design tradeoffs, configuration reference, and
release notes, see [Architecture.md](Architecture.md).

## Privacy

All indexes are local. Metadata and embeddings are stored in SQLite, and BM25
data is stored in Tantivy.

When an external agent reads selected chunks and includes them in its context,
those excerpts may be sent to the agent's model provider depending on your agent
configuration. `obsidian-kb` itself does not make hosted LLM calls.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
