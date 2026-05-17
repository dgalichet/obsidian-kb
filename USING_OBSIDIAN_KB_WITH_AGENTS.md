# Using obsidian-kb with LLM agents

Use this document to guide an LLM agent, an MCP client, or an agentic coding
tool when it needs to initialize, index, maintain, or query an Obsidian vault
with `obsidian-kb`.

`obsidian-kb` means **Obsidian Knowledge Base**. It is a local-first retrieval
layer for Obsidian Markdown notes. The goal is not to load the whole vault into
an LLM context. The goal is to index the vault locally, retrieve a few relevant
chunks, and answer with citations.

## Agent Contract

Non-negotiable rules:

- For content questions about the vault, always run `obsidian-kb search`, or
  the MCP `search` tool when using `obsidian-kb mcp`, first.
- Never read the whole vault.
- Do not use recursive `cat`, `rg`, `grep`, `find`, `fd`, or bulk file reads to
  answer vault content questions before search has identified specific sources.
- Read only the top relevant chunks returned by search.
- Prefer `search --include-text --max-chars 1200 --json` for compact first-pass
  context.
- Use `show <chunk-id> --json` only when the search result text is insufficient.
- Prefer MCP mode for long-lived agent sessions with repeated vector or hybrid
  searches, so the local embedding model can stay warm between requests.
- Cite note paths, headings, and line ranges in every content summary.
- Do not modify Obsidian notes unless the user explicitly asks.
- Do not add OpenAI, Claude, ChatGPT, or other hosted LLM API calls inside
  `obsidian-kb`.
- Keep indexing, embeddings, and retrieval local.
- Be explicit that excerpts read by an external agent may be sent to that
  agent's model provider depending on the agent configuration.

The useful behavior comes from selection, not volume.

## What The Tool Does

`obsidian-kb` indexes an Obsidian vault as Markdown:

```text
Obsidian Markdown vault
  -> frontmatter, properties, aliases, tags, headings, wikilinks, and backlinks
  -> heading-aware chunks
  -> SQLite metadata, structured filters, and local embeddings
  -> Tantivy BM25 index
  -> local FastEmbed vector embeddings
  -> BM25, vector, or hybrid search
  -> Reciprocal Rank Fusion
  -> optional shallow graph expansion
  -> cited chunks for the agent
```

Do not describe this as full GraphRAG. `obsidian-kb` does not extract entities,
create communities, generate global summaries, or perform LLM graph reasoning.
It only uses Obsidian links and backlinks as a shallow retrieval aid.

## Binary Resolution

Prefer the installed binary:

```bash
obsidian-kb
```

If it is not in `PATH`, try:

```bash
${HOME}/.cargo/bin/obsidian-kb
```

From a checkout of this repository, install or update the local binary with:

```bash
cargo install --path . --force
```

After installation, do not require users or agents to call
`./target/release/obsidian-kb`.

## Golden Path: Index A Vault

Preferred setup keeps `.obsidian-kb.toml` inside the vault. This makes later
commands simple and avoids ambiguity about where the config lives.

```bash
cd /path/to/ObsidianVault
obsidian-kb init --vault .
obsidian-kb index
obsidian-kb doctor
obsidian-kb stats --json
```

Then run three smoke-test searches:

```bash
obsidian-kb search "exact project or note name" --mode bm25 --top 5 --json
obsidian-kb search "vague conceptual question" --mode vector --top 5 --json
obsidian-kb search "broad topic to explore" --mode hybrid --expand-graph --top 8 --json
```

Read only a few returned chunks:

```bash
obsidian-kb show <chunk-id> --json
```

Report:

- vault path;
- config path;
- index stats;
- doctor issues and warnings;
- whether BM25, vector, and hybrid searches return useful chunks;
- note structure problems such as huge notes, noisy imports, missing headings, or
  broken links.

Do not change notes during this setup unless the user explicitly approves it.

## Config Placement And Resolution

Important: `obsidian-kb init` writes `.obsidian-kb.toml` in the current working
directory. Therefore, for a vault-local config, run `init` from inside the
vault:

```bash
cd /path/to/ObsidianVault
obsidian-kb init --vault .
```

If a config is stored somewhere else, pass it explicitly:

```bash
obsidian-kb --config /path/to/.obsidian-kb.toml index
obsidian-kb --config /path/to/.obsidian-kb.toml search "query" --json
```

Config lookup order for commands that load an existing config:

1. `--config /path/to/.obsidian-kb.toml`
2. `--vault /path/to/vault`, which expects
   `/path/to/vault/.obsidian-kb.toml`
3. `.obsidian-kb.toml` in the current working directory

Use `--vault /path/to/Vault` only when the config is actually located at
`/path/to/Vault/.obsidian-kb.toml`. Otherwise use `--config`.

## Recommended Starting Config

The default config is a good starting point:

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

[mcp]
idle_unload_seconds = 600
preload_embedder = false

[doctor.unresolved_links]
allow_forward_links = false
ignore_targets = []
ignore_globs = []
```

Tune only when there is evidence:

- chunks too large or mixed: reduce `chunk_target_chars` and `max_chunk_chars`;
- weak recall: increase `bm25_candidates` or `vector_candidates`;
- noisy graph expansion: reduce `graph_max_neighbors` or avoid
  `--expand-graph`;
- recurring false-positive unresolved links: configure
  `[doctor.unresolved_links]`;
- shared model cache needed: set `embeddings.cache_dir`;
- repeated MCP semantic searches start cold: call the MCP `warmup` tool or set
  `mcp.preload_embedder = true`;
- MCP memory should be released faster: reduce `mcp.idle_unload_seconds`.

FastEmbed may download a local embedding model on first embedding build. The
download is for local model files, not a hosted LLM call. If embeddings are
temporarily unavailable, an agent may run:

```bash
obsidian-kb index --no-embeddings
```

Then use `--mode bm25` until embeddings can be built.

## MCP Mode For Agents

Use MCP mode when the agent platform can keep a long-lived stdio MCP server and
will run several searches in one session:

```bash
obsidian-kb --config /path/to/.obsidian-kb.toml mcp
```

If the config is vault-local and the working directory is the vault, this is
enough:

```bash
obsidian-kb mcp
```

MCP mode exposes these tools:

- `search`: run BM25, vector, or hybrid search with the same options as the CLI;
- `show`: read one selected chunk by ID;
- `tags`: list indexed tags, optionally filtered by prefix;
- `properties`: list indexed frontmatter property keys, or values for one key;
- `stats`: inspect indexed vault statistics;
- `warmup`: load the local embedding model and embeddings into memory;
- `unload`: drop the warm embedding cache when it is no longer needed;
- `status`: inspect whether the warm cache is loaded.

For exact one-off lookups, the CLI is usually enough:

```bash
obsidian-kb search "exact note or command" --mode bm25 --top 5 --json
```

For repeated semantic or hybrid retrieval, prefer MCP:

1. Start `obsidian-kb mcp`.
2. Call `warmup` at the beginning of a retrieval-heavy session, or set
   `mcp.preload_embedder = true`.
3. Call `search` for vector or hybrid queries.
4. Call `show` only for selected chunks.
5. Call `unload` when the session is done if memory should be released
   immediately.

`mcp.idle_unload_seconds` controls automatic cache unloading after inactivity.
The MCP process stays alive; only the warm embedding model and embeddings are
dropped. Set it to `0` only when automatic unloading should be disabled.

MCP mode does not add hosted LLM calls. Embeddings and retrieval remain local.
However, excerpts returned to an external agent may still be sent to that
agent's model provider depending on the agent configuration.

## Indexing Policy

For the first setup:

```bash
obsidian-kb index
obsidian-kb doctor
```

For routine refresh after notes changed:

```bash
obsidian-kb index
obsidian-kb doctor
```

Routine indexing refreshes SQLite/Tantivy and reuses embeddings whose chunk
content hash is unchanged. The legacy `--changed-only` flag is accepted for
compatibility, but it is not a true changed-only SQLite/Tantivy indexer.

For diagnostics:

```bash
obsidian-kb stats --json
obsidian-kb doctor --json
obsidian-kb graph "Note name" --depth 1 --json
```

Do not rebuild automatically before every answer. Search first. Refresh the
index only when:

- the user mentions recent note changes;
- expected recent notes are missing from search results;
- `doctor` reports missing indexes, count mismatches, or config problems;
- smoke tests return obviously stale results.

Use a full rebuild only when:

- chunking settings changed;
- the embedding model changed;
- the config changed in a way that affects indexed content;
- SQLite or Tantivy indexes appear corrupted;
- the user explicitly asks for a full rebuild.

Full rebuild:

```bash
obsidian-kb index --rebuild
obsidian-kb doctor
```

## Choosing Search Mode

Use `--mode bm25` for exact terms:

- note titles;
- project names;
- commands;
- error messages;
- APIs;
- class or function names;
- acronyms;
- file names.

```bash
obsidian-kb search "Service Connect TLS App Mesh" --mode bm25 --top 5 --json
```

Use `--mode vector` for vague or semantic search:

- half-remembered ideas;
- synonyms;
- conceptual reformulations;
- questions where exact vocabulary is unknown.

```bash
obsidian-kb search "how to avoid overloading an agent context" --mode vector --top 5 --json
```

Use `--mode hybrid --expand-graph` for conceptual questions that may benefit
from both meaning, exact terms, and linked notes:

```bash
obsidian-kb search "Obsidian as a local RAG knowledge base" --mode hybrid --expand-graph --top 8 --json
```

Graph expansion is a recall aid, not proof. Read a graph-expanded neighbor only
when its path, title, heading, tags, or snippet is clearly relevant.

## Structured Filters

Use structured filters when the user mentions a known tag or frontmatter
property such as status, project, type, source, author, date, or priority.
Filters narrow results; they do not replace semantic or exact search.

Discover available filter values before guessing:

```bash
obsidian-kb tags --json
obsidian-kb tags --prefix ai --json
obsidian-kb properties --json
obsidian-kb properties --key status --json
```

Apply filters with search:

```bash
obsidian-kb search "retrieval" --tag ai/context --property status=active --json
obsidian-kb search --property type=book --property status=reading --json
obsidian-kb search --property 'created>=2026-01-01' --json
```

In MCP mode, use the `tags` and `properties` tools for discovery, then pass
filters to `search` as arrays:

```json
{ "prefix": "ai" }
{ "key": "status" }
{ "query": "retrieval", "mode": "hybrid", "tags": ["ai/context"], "properties": ["status=active"] }
```

For clients that handle single values more naturally, the MCP `search` tool also
accepts `tag` and `property` aliases:

```json
{ "query": "retrieval", "mode": "bm25", "tag": "ai/context", "property": "status=active" }
```

Rules:

- Repeat `--tag` and `--property` to combine filters with AND semantics.
- Use `--property KEY=VALUE` for exact matches.
- Use quoted comparison filters for dates or numbers: `KEY>=VALUE`, `KEY<=VALUE`, `KEY>VALUE`, `KEY<VALUE`.
- Use `KEY!=VALUE` only when you want notes where that property key exists and does not have that value.
- Prefer filters for metadata constraints; do not put metadata terms into the free-text query unless they are also part of the conceptual question.

## Compact Context For Agents

Preferred first pass:

```bash
obsidian-kb search "query" --mode hybrid --expand-graph --top 5 --include-text --max-chars 1200 --json
```

Rules:

- Use `--include-text` only with `--json`.
- Keep `--max-chars` bounded for normal agent workflows.
- Use `--max-chars 0` only when the user explicitly needs the full chunk text.
- Start with 3 to 5 chunks for simple questions.
- Use up to 10 chunks for cross-cutting questions.
- Limit graph-expanded neighbor reads to 2 unless clearly needed.
- Reformulate the query before expanding context aggressively.

Use `show` when the search output is not enough:

```bash
obsidian-kb show <chunk-id> --json
```

Open a full Markdown file only after search has identified the file and there is
a clear reason, such as editing, checking nearby context, or verifying exact
wording outside the chunk.

## Search Result Fields

JSON search results are designed for explainable retrieval. Use these fields:

- `final_rank` and `final_score`;
- `bm25_rank` and `bm25_score`;
- `vector_rank` and `vector_score`;
- `graph_boost`;
- `path`;
- `title`;
- `heading_path`;
- `start_line` and `end_line`;
- `tags`;
- `snippet`;
- `chunk_id`;
- `text`, when `--include-text` is set.

A high score means likely relevance, not factual certainty. Verify facts from
the chunk text before answering.

## Answering Workflow

For a content question:

1. Classify the question: exact, vague, or conceptual.
2. If the user names metadata constraints, discover tags/properties first.
3. Run one targeted `obsidian-kb search`, adding structured filters when useful.
4. Inspect paths, headings, snippets, line ranges, and scores.
5. Use included text if enough; otherwise read selected chunks with `show`.
6. If results are weak, reformulate once or switch search mode.
7. Answer from the retrieved excerpts.
8. Cite path, heading, and line range.
9. State gaps when retrieved sources do not fully answer the question.

Do not fabricate citations. Prefer a partial sourced answer over a broad
unsourced answer.

Useful retrieval summary format:

```text
Queries:
- "..."

Modes:
- hybrid --expand-graph

Chunks consulted:
- <chunk-id> | path | heading | lines <start>-<end>

Facts:
- ...

Uncertainties:
- ...

Sources to cite:
- path | heading | lines
```

## Ingestion Guidance

`obsidian-kb` indexes Markdown. PDFs, scans, HTML exports, office documents, and
image-heavy sources should be converted to clean Markdown before indexing.

For an ingestion agent:

1. Identify source type: Markdown, text PDF, OCR scan, image, table, web export,
   or office document.
2. Convert to structured Markdown.
3. Remove noise: navigation, duplicated footers, ads, irrelevant legal blocks,
   broken OCR fragments, repeated text.
4. Preserve structure: title, headings, lists, tables, code blocks, quotations,
   and source references.
5. Describe useful images in Markdown when they contain information.
6. Preserve provenance: source URL, local path, author, date, and import date
   when available.
7. Avoid over-summarizing at ingestion time. Notes must remain verifiable.

Do not blindly route ingestion through an LLM if it increases cost, noise, or
errors. For a few important documents, manual cleanup can be better than a large
automated conversion.

## Vault Quality Hints

The vault should remain useful to humans. A simple structure is enough:

```text
Vault/
  Inbox/
  Sources/
  Notes/
  Projects/
  Index/
```

Good signals:

- each note has a clear title;
- headings divide ideas into searchable sections;
- tags are stable and not overly granular;
- frontmatter properties use stable keys and predictable scalar values;
- aliases cover exact names, acronyms, and common variants;
- wikilinks represent meaningful relationships;
- large imported documents are split or structured with headings.

Bad signals:

- raw folders full of uncleaned imports;
- pages without headings;
- repeated copies of the same document;
- long encyclopedia-style notes with many unrelated topics;
- broken OCR or layout artifacts;
- contradictory duplicates.

Agents may report these problems and propose fixes, but must not rewrite the
vault without approval.

## Maintenance Checklist

When the vault changes:

```bash
obsidian-kb index
obsidian-kb doctor
```

When config, chunking, or embedding model changes:

```bash
obsidian-kb index --rebuild
obsidian-kb doctor
```

When search quality is poor:

1. Run `doctor --json`.
2. Run `stats --json`.
3. Try one BM25 query with an exact known term.
4. Try one vector query with a paraphrase.
5. Try one hybrid graph query for a broad topic.
6. Inspect whether failures come from config, missing embeddings, note structure,
   an index that needs refresh, or a bad query.

Do not compensate for poor retrieval by loading more and more files.

## Agent Role Split

When the platform supports sub-agents, keep responsibilities separate.

Orchestrator:

- understands the user request;
- chooses search mode;
- asks for indexing or diagnostics when needed;
- produces final answer with citations.

Librarian:

- manages `.obsidian-kb.toml`;
- runs `index`, `doctor`, and `stats`;
- reports freshness, exclusions, and embedding status;
- does not summarize domain content unless needed for diagnostics.

Retriever:

- formulates targeted queries;
- reads only selected chunks;
- returns paths, headings, line ranges, facts, and uncertainties.

Ingestor:

- converts documents to clean Markdown;
- identifies structure and provenance;
- modifies the vault only with explicit approval.

Answerer:

- answers only from retrieved excerpts;
- separates sourced facts, hypotheses, and gaps;
- requests one targeted additional search if excerpts are insufficient.

## Short Prompts

Retrieval agent:

```text
You are a retrieval agent for an Obsidian vault indexed by obsidian-kb. Never
read the whole vault. For content questions, start with obsidian-kb search, or
the MCP search tool when obsidian-kb mcp is available. Use bm25 for exact names,
vector for vague concepts, and hybrid --expand-graph for conceptual questions.
Prefer MCP mode for repeated vector or hybrid searches so the local embedding
model stays warm. Prefer search --include-text --max-chars 1200 --json for
compact context. Use obsidian-kb show only for selected chunks. Return chunk
IDs, paths, headings, line ranges, facts, and uncertainties.
```

Answer agent:

```text
Answer only from excerpts provided by retrieval. Cite paths, headings, and line
ranges. Separate sourced facts from hypotheses and gaps. If excerpts are
insufficient, request one targeted additional search.
```

Librarian agent:

```text
Maintain an Obsidian vault index with obsidian-kb. Prefer a vault-local
.obsidian-kb.toml created by running init from inside the vault. Run index,
doctor, stats, and smoke-test searches. When MCP mode is used, configure
mcp.idle_unload_seconds, use warmup/status/unload for cache diagnostics, and
report whether the embedding cache stays warm. Report config, freshness,
embedding, link, and note-structure issues. Do not modify notes without
approval.
```

Ingestion agent:

```text
Prepare sources for an Obsidian vault. Convert them to clean, structured
Markdown with title, headings, provenance, and useful source references. Remove
layout noise and OCR artifacts. Do not over-summarize. Do not modify the vault
without explicit approval.
```

## Limits

`obsidian-kb` must not become:

- an automatic vault rewriter;
- a hosted LLM pipeline;
- a remote vector database;
- a system of unverifiable global summaries;
- a reason to load hundreds of pages into context.

Keep the source of truth in Obsidian Markdown. Keep retrieval local. Keep agent
context small and cited.
