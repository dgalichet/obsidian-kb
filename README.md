<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/obsidian-kb-logo.png">
    <source media="(prefers-color-scheme: light)" srcset="assets/obsidian-kb-logo-light.png">
    <img src="assets/obsidian-kb-logo-light.png" alt="obsidian-kb logo" width="720">
  </picture>
</p>

# obsidian-kb

**Search your Obsidian vault like a knowledge base. Keep it local.**

Website: [dgalichet.github.io/obsidian-kb](https://dgalichet.github.io/obsidian-kb/)

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

### macOS Apple Silicon

With Homebrew:

```bash
brew install dgalichet/tap/obsidian-kb
```

Or download the matching macOS archive from the
[latest GitHub Release](https://github.com/dgalichet/obsidian-kb/releases/latest)
and put `obsidian-kb` on your `PATH`.

### Linux x86_64

Release archives for Linux are named by Rust target triple. Replace `vX.Y.Z`
with a release tag that contains the Linux archive, then extract the whole
directory and put a wrapper on your `PATH`:

```bash
version="vX.Y.Z"
tmpdir="$(mktemp -d)"
curl -L \
  "https://github.com/dgalichet/obsidian-kb/releases/download/${version}/obsidian-kb-x86_64-unknown-linux-gnu.tar.gz" \
  -o "${tmpdir}/obsidian-kb-linux-x86_64.tar.gz"

sudo mkdir -p /opt/obsidian-kb
sudo tar -xzf "${tmpdir}/obsidian-kb-linux-x86_64.tar.gz" \
  -C /opt/obsidian-kb \
  --strip-components=1
sudo tee /usr/local/bin/obsidian-kb >/dev/null <<'EOF'
#!/usr/bin/env sh
exec /opt/obsidian-kb/obsidian-kb "$@"
EOF
sudo chmod +x /usr/local/bin/obsidian-kb

obsidian-kb --help
```

Keep the extracted files together under `/opt/obsidian-kb`; the archive may
include runtime libraries that must stay next to the executable.

### Windows x86_64

Release archives for Windows are named by Rust target triple. Replace `vX.Y.Z`
with a release tag that contains the Windows archive. In PowerShell:

```powershell
$Version = "vX.Y.Z"
$InstallDir = "$env:LOCALAPPDATA\Programs\obsidian-kb"
$Archive = "$env:TEMP\obsidian-kb-windows-x86_64.tar.gz"

Invoke-WebRequest `
  -Uri "https://github.com/dgalichet/obsidian-kb/releases/download/$Version/obsidian-kb-x86_64-pc-windows-msvc.tar.gz" `
  -OutFile $Archive

New-Item -ItemType Directory -Force $InstallDir | Out-Null
tar -xzf $Archive -C $InstallDir --strip-components 1

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ([string]::IsNullOrWhiteSpace($UserPath)) {
  [Environment]::SetEnvironmentVariable("Path", $InstallDir, "User")
} elseif (($UserPath -split ";") -notcontains $InstallDir) {
  [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
}
$env:Path = "$env:Path;$InstallDir"

obsidian-kb.exe --help
```

Open a new terminal if `obsidian-kb.exe` is not found after updating the user
`PATH`. Keep the extracted files together in `$InstallDir`; the archive may
include runtime DLLs that must stay next to the executable.

### From Source

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

The JSON output is grouped by indexed document. Each result includes its best
chunk, matched chunk count, matched chunks with headings, line ranges, optional
PDF page ranges, snippets, chunk IDs, ranks, scores, document kind, and graph
boosts.

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

## PDF Attachments

PDF indexing is local and opt-in. Enable it in `.obsidian-kb.toml`:

```toml
[index.pdf]
enabled = true
max_file_size_mb = 50
```

When enabled, `index` also scans non-excluded `.pdf` files, extracts embedded
text locally with `lopdf`, chunks each text-bearing page as `Page N`, and stores
the chunks alongside Markdown chunks for BM25, vector, and hybrid search. PDF
results use `document_kind = "pdf"` and include `start_page`/`end_page` fields
in JSON output. Scanned image-only PDFs need OCR first; this feature does not
call hosted APIs or write extracted text back to the vault.

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
obsidian-kb show <chunk-id> <chunk-id> --json
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

Ready-to-copy integration recipes are available in
[docs/agent-recipes.md](docs/agent-recipes.md): Claude Desktop MCP,
Codex/CLI agents, Cursor, Continue, shell scripts, and a reusable
`search first, read second` system prompt.

For the complete agent and MCP usage guide, see
[docs/using-obsidian-kb-with-agents.md](docs/using-obsidian-kb-with-agents.md).

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
Published vault-size benchmark results and the reproducible synthetic benchmark
runner are documented in [docs/benchmarks.md](docs/benchmarks.md).

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

Browser CORS access is restricted to configured origins. The default is the
Obsidian desktop origin:

```toml
[serve]
cors_allowed_origins = ["app://obsidian.md"]
```

Requests without an `Origin` header, such as local CLI and native clients, are
not blocked. To allow a local browser app, add its exact origin including the
port, for example `http://127.0.0.1:3000`. Use `*` only when you intentionally
want any browser page to read from the local service.

## What It Is Not

`obsidian-kb` is intentionally small in scope:

- not a hosted AI product;
- not a hosted vector database;
- not a LangChain or LlamaIndex wrapper;
- not a full GraphRAG system;
- not an automatic note writer.

For implementation details, design tradeoffs, configuration reference, and
release notes, see [docs/architecture.md](docs/architecture.md).

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
