# Codex Instructions for Building a Local Knowledge Base from Obsidian

This file describes a procedure an agent such as Codex can follow to turn an
Obsidian vault into a useful local knowledge base for LLM-assisted work without
reading the whole vault.

This procedure keeps indexing, embeddings, and retrieval local. The
`obsidian-kb` tool itself does not call hosted LLM APIs and does not upload
vault content.

When an external agent such as Codex reads selected chunks and includes them in
its context, those selected excerpts may be sent to the model provider depending
on the agent configuration. Therefore, retrieval must remain selective and
minimal.

The core idea is straightforward: Obsidian provides Markdown files, tags, and
wikilinks. Obsidian alone is not RAG. The useful knowledge base comes from the
retrieval layer added by `obsidian-kb`: parsing, chunking, BM25 indexing, local
embeddings, rank fusion, limited graph expansion, and disciplined selective
reading.

## Objective

Build a local knowledge base that lets an agent:

- retrieve a few relevant passages instead of loading the whole vault;
- distinguish exact search, conceptual search, and vague semantic search;
- cite the file paths, titles, and headings it consulted;
- keep context short, clean, and verifiable;
- keep Obsidian Markdown files as the source of truth;
- write only local index files under `.obsidian-kb/`, unless the user explicitly
  asks to modify the vault.

## Binary Location

Assume the locally installed binary is:

```bash
${HOME}/.cargo/bin/obsidian-kb
```

If `${HOME}/.cargo/bin` is in `PATH`, use the short command:

```bash
obsidian-kb
```

Otherwise, invoke the binary with its full installed path:

```bash
${HOME}/.cargo/bin/obsidian-kb
```

The preferred local installation command from the project root is:

```bash
cargo install --path . --force
```

Do not require callers to use `./target/release/obsidian-kb` once the binary has
been installed.

## Target Architecture

Expected flow:

```text
Source documents
  -> cleaning / Markdown conversion when needed
  -> structured Obsidian vault
  -> obsidian-kb init
  -> Markdown, frontmatter, tags, aliases, and wikilink parsing
  -> heading-aware chunking
  -> local SQLite storage
  -> Tantivy BM25 index
  -> local FastEmbed embeddings
  -> hybrid BM25 + vector search
  -> Reciprocal Rank Fusion
  -> limited graph expansion through wikilinks/backlinks
  -> selective reading of top chunks
  -> concise answer with citations
```

Do not present this system as full GraphRAG. `obsidian-kb` does not extract
entities, does not generate communities, does not create global summaries, and
does not call hosted LLMs.

## Non-Negotiable Rules

- For content questions about the vault, always start with `obsidian-kb search`.
- For diagnostics or maintenance, start with `doctor`, `stats`, or `index`.
- Never read the whole vault to answer a question.
- Read only the best chunks returned by search.
- Do not use `cat`, `rg`, `grep`, `fd`, `find`, recursive directory reads, or
  bulk file opening to answer content questions unless `obsidian-kb search` has
  first identified a specific file or chunk that needs inspection.
- Do not modify Obsidian notes unless explicitly asked.
- Do not add OpenAI, Claude, ChatGPT, or other hosted LLM calls inside
  `obsidian-kb`.
- Keep indexing, embeddings, and retrieval local.
- Be explicit that selected chunks read by an external agent may be sent to that
  agent's model provider depending on configuration.
- Cite file paths and headings in every content summary.
- Prefer a partial sourced answer over a broad unsourced answer.

## Initializing a Vault

From the local machine:

```bash
obsidian-kb init --vault /path/to/ObsidianVault
obsidian-kb index --vault /path/to/ObsidianVault
obsidian-kb doctor --vault /path/to/ObsidianVault
obsidian-kb stats --vault /path/to/ObsidianVault --json
```

If `obsidian-kb` is not in `PATH`, use:

```bash
${HOME}/.cargo/bin/obsidian-kb init --vault /path/to/ObsidianVault
${HOME}/.cargo/bin/obsidian-kb index --vault /path/to/ObsidianVault
${HOME}/.cargo/bin/obsidian-kb doctor --vault /path/to/ObsidianVault
${HOME}/.cargo/bin/obsidian-kb stats --vault /path/to/ObsidianVault --json
```

The `init` command creates `.obsidian-kb.toml`. The index files live under
`.obsidian-kb/`, not inside the Markdown notes. Relative paths in the config are
resolved from the configuration directory.

If the current working directory contains `.obsidian-kb.toml`, commands can be
run without `--vault`:

```bash
cd /path/to/directory/containing/config
obsidian-kb search "local RAG with Obsidian" --mode hybrid --expand-graph
obsidian-kb index
obsidian-kb doctor
```

Config resolution order:

1. `--config /path/to/.obsidian-kb.toml`, when provided.
2. `--vault /path/to/vault`, which resolves
   `/path/to/vault/.obsidian-kb.toml`.
3. `.obsidian-kb.toml` in the current working directory.

## Recommended Configuration

Expected starting configuration:

```toml
[vault]
exclude_globs = [
  ".obsidian/**",
  ".obsidian-kb/**",
  ".trash/**",
  "Templates/**",
  "**/*.excalidraw.md"
]

[index]
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

[doctor.unresolved_links]
allow_forward_links = false
ignore_targets = []
ignore_globs = []
```

When using E5-style embedding models, encode user queries with the prefix
`query: ` and document chunks with the prefix `passage: `.

Example:

- query embedding input: `query: how to avoid overloading an agent context`
- document embedding input: `passage: <chunk content>`

Tune only when there is a measured problem:

- chunks too long: reduce `chunk_target_chars` and `max_chunk_chars`;
- weak recall: increase `bm25_candidates` or `vector_candidates`;
- noisy graph expansion: reduce `graph_max_neighbors` or avoid
  `--expand-graph`;
- embeddings unavailable: temporarily index with `--no-embeddings`, then index
  again when FastEmbed is available.

## Index Freshness

Before answering a content question, do not rebuild the index automatically.

However, if:

- search results are obviously stale;
- recently created notes are missing;
- the user explicitly mentions recent changes;
- `doctor` reports stale files;

then run:

```bash
obsidian-kb index --changed-only
```

Then repeat the search.

Do not run `--rebuild` unless:

- the config changed;
- chunking settings changed;
- the embedding model changed;
- the Tantivy or SQLite index appears corrupted;
- the user explicitly requests a full rebuild.

## Document Cleaning and Ingestion

`obsidian-kb` indexes Markdown. PDFs, scans, HTML exports, office documents, and
image-heavy sources must be converted cleanly before they enter the vault.

Procedure for an ingestion agent:

1. Identify the source type: native Markdown, text PDF, OCR scan, image, table,
   web export, or office document.
2. Convert to structured Markdown before indexing.
3. Remove noise: navigation, repeated footers, ads, irrelevant legal sections,
   duplicated text, and corrupted OCR fragments.
4. Preserve logical structure: headings, subheadings, lists, tables, code
   blocks, and useful quotations.
5. Describe useful images in Markdown when they contain information.
6. Preserve source references: origin, date, author, local path, or source URL
   when available.
7. Avoid over-aggressive summaries during ingestion: the note must remain
   verifiable.

Do not blindly automate conversion through an LLM if doing so adds token cost,
noise, or errors. For a small number of critical documents, targeted manual
cleaning can be better.

## Vault Organization

The vault should remain readable by a human. A simple structure is enough:

```text
Vault/
  Inbox/
  Sources/
  Notes/
  Projects/
  Index/
```

Good practices:

- each note should have an explicit title;
- headings should divide ideas, not merely decorate the page;
- tags should remain stable and few;
- aliases should cover exact names, acronyms, and useful variants;
- wikilinks should represent useful relationships, not every important word;
- avoid huge notes without headings.

Bad signals:

- `raw/` folders full of uncleaned files;
- pages without titles or structure;
- repeated copies of the same document;
- encyclopedia-style notes with tens of thousands of characters;
- imported documents with broken layout or noisy OCR.

## Agent Roles

When the platform supports it, separate responsibilities into agents or
sub-agents with isolated context. The goal is to prevent the main agent from
loading too much context.

### Orchestrator Agent

Responsibilities:

- understand the user request;
- choose the search mode;
- delegate bounded tasks to sub-agents;
- receive short summaries;
- produce the final answer with citations.

It never reads the whole vault.

### Ingestor Agent

Responsibilities:

- convert and clean source documents before adding them to the vault;
- propose Markdown structure;
- detect duplicates, noise, bad OCR, and irrelevant sections;
- modify the vault only when the user explicitly asked for it.

Expected output:

- list of processed documents;
- proposed paths;
- quality risks;
- performed or pending operations.

### Librarian Agent

Responsibilities:

- maintain `.obsidian-kb.toml`;
- run `obsidian-kb index`, `doctor`, and `stats`;
- verify that the index is fresh;
- report exclusions, config errors, and missing embeddings.

It should not summarize domain content unless needed for diagnostics.

### Retrieval Agent

Responsibilities:

- formulate one or more queries;
- use the correct search mode;
- read only necessary chunks with `show`;
- return a short sourced summary.

Expected output:

- queries run;
- mode used;
- chunk IDs consulted;
- paths and headings;
- relevant facts;
- uncertainties.

### Answer Agent

Responsibilities:

- merge retrieval results;
- distinguish sourced facts, hypotheses, and gaps;
- answer clearly without context bloat;
- cite files and headings.

## Choosing the Search Mode

Use `--mode bm25` for:

- exact names;
- commands;
- errors;
- APIs;
- classes;
- acronyms;
- file paths;
- exact titles.

Example:

```bash
obsidian-kb search "Service Connect TLS App Mesh" --mode bm25 --top 5 --json
```

Use `--mode vector` for:

- vague questions;
- concepts without exact vocabulary;
- reformulation;
- nearby ideas.

Example:

```bash
obsidian-kb search "how to avoid overloading an agent context" --mode vector --top 5 --json
```

Use `--mode hybrid --expand-graph` for:

- conceptual questions;
- topic exploration;
- cases that need exact words, meaning, and linked notes.

Example:

```bash
obsidian-kb search "Obsidian as a local RAG knowledge base" --mode hybrid --expand-graph --top 8 --json
```

Graph expansion is a recall aid, not a source of authority. A neighbor note
should only be read when its title, heading, tags, or snippet make it relevant
to the user question.

For compact agent context, add bounded source text directly to JSON results:

```bash
obsidian-kb search "Obsidian as a local RAG knowledge base" --mode hybrid --expand-graph --top 5 --include-text --max-chars 1200 --json
```

Use `--include-text` only with `--json`. `--max-chars` limits included text per
chunk; `--max-chars 0` includes the full chunk text. Prefer a bounded value for
agent workflows unless the user explicitly needs full chunk content.

Add `--vault /path/to/Vault` only when the current directory does not contain
the relevant `.obsidian-kb.toml` and no `--config` path is provided.

## Reading Chunks

`search --json` returns enough metadata to select and cite likely sources:
paths, headings, line ranges, tags, snippets, chunk IDs, and ranking evidence.
When `--include-text` is used, it can also return compact chunk text.

Use `show` after search when:

- the snippet and included text are insufficient for a sourced answer;
- precise editing or verification needs the full chunk;
- the user asks for exact wording beyond the included text limit;
- the answer depends on context near the chunk boundaries.

```bash
obsidian-kb show <chunk-id> --json
```

Reading discipline:

- start with the top 3 to 5 chunks;
- use `search --include-text --max-chars 1200 --json` for a compact first pass;
- increase only if the answer remains ambiguous;
- prefer several short chunks over a full large document;
- stop reading when the necessary facts are sufficiently verified;
- do not open a full file unless there is a clear reason.

Every answer should cite at least:

- note path;
- heading or heading path;
- extracted fact.

## Context Management

Long context can degrade answer quality. Apply these default limits:

- maximum 5 chunks for a simple question;
- maximum 10 chunks for a cross-cutting question;
- maximum 2 graph-expanded neighbor notes, unless explicitly needed;
- no full-document insertion into the main prompt;
- no global vault summary without staged retrieval.

If results are poor, reformulate the query instead of loading more documents.

## Retrieval Explainability

JSON search results expose fields such as:

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
- `text`, only when `--include-text` is set;
- `chunk_id`.

Use these fields to explain why a passage was read. Do not confuse final score
with factual certainty: a good rank signals likely relevance, not truth.

## Reranking

In the current project, reranking is Reciprocal Rank Fusion between BM25 and
vector search. This is intentionally simple, local, and explainable.

Do not claim that a neural cross-encoder reranker already exists in
`obsidian-kb`.

If a true reranker is added later, it must respect these constraints:

- local-first;
- no hosted LLM API;
- explainable results;
- tests on a small fixture vault;
- clear fallback to BM25/vector/RRF.

## Workflow for Answering a Question

Standard procedure:

1. Classify the question: exact, vague, or conceptual.
2. Run `obsidian-kb search` with the appropriate mode.
3. Inspect titles, headings, line ranges, tags, snippets, and scores.
4. If compact context is enough, use `--include-text`; otherwise read the best
   chunks with `obsidian-kb show`.
5. Run a more precise query if the chunks are insufficient.
6. Answer with citations.
7. Mention limitations when sources do not cover the full question.

Internal output template for the Retrieval Agent:

```text
Queries:
- "..."

Mode:
- hybrid --expand-graph

Chunks read:
- <chunk-id> - path - heading

Search metadata:
- lines <start>-<end> - tags [...]

Facts:
- ...

Uncertainties:
- ...

Sources to cite:
- path - heading
```

## Workflow for Building the Initial Knowledge Base

Complete procedure:

1. Confirm the vault path.
2. Run `obsidian-kb init --vault <vault>` if the config does not exist.
3. Check `.obsidian-kb.toml`, especially `exclude_globs`.
4. Run `obsidian-kb index --vault <vault>`.
5. Run `obsidian-kb doctor --vault <vault>`.
6. Run `obsidian-kb stats --vault <vault> --json`.
7. Run three smoke-test searches:
   - an exact name with `--mode bm25`;
   - a vague question with `--mode vector`;
   - a conceptual question with `--mode hybrid --expand-graph`.
8. Read chunks returned by each smoke test.
9. Report problems: poorly structured notes, oversized chunks, missing
   embeddings, incorrect exclusions, or noisy results.
10. Propose targeted fixes without modifying the vault unless approved.

## Maintenance

When the vault changes:

```bash
obsidian-kb index --changed-only
obsidian-kb doctor
```

When config, chunking, or the embedding model changes:

```bash
obsidian-kb index --rebuild
obsidian-kb doctor
```

For diagnostics:

```bash
obsidian-kb stats --json
obsidian-kb graph "Note name" --depth 1 --json
obsidian-kb doctor --json
```

Add `--vault /path/to/Vault` to these commands only when running outside the
directory that contains the relevant `.obsidian-kb.toml`.

## Quality Criteria

A knowledge base is usable when:

- `doctor` reports no blocking issue;
- exact searches find expected names;
- vector searches find reformulated ideas;
- hybrid searches return coherent chunks;
- snippets are readable;
- headings provide useful context;
- graph-expanded neighbor notes are useful and few;
- final answers cite consulted sources.

It is not yet healthy when:

- the agent must open many full files;
- top results are noise;
- chunks mix several topics without headings;
- imported documents contain broken layout;
- citations are impossible or vague;
- the same information exists in multiple contradictory notes.

## Limits to Respect

`obsidian-kb` is a local retrieval layer. It must not become:

- a tool that automatically rewrites the vault;
- a pipeline of hosted LLM API calls;
- a remote vector database;
- a system of unverifiable global summaries;
- an excuse to load hundreds of pages into context.

The value comes from selection, not volume.

## Short Prompts for Agents

System prompt for a retrieval agent:

```text
You are a retrieval agent for an Obsidian vault indexed by obsidian-kb.
Never read the whole vault. For content questions about the vault, always start
with obsidian-kb search. For maintenance, freshness, or diagnostic tasks, start
with doctor, stats, or index as appropriate.
Use bm25 for exact names, vector for vague questions, and hybrid --expand-graph
for conceptual questions. For compact context, use search --include-text
--max-chars 1200 --json. Read only the best chunks with obsidian-kb show when
the search text or snippet is insufficient. Return a short summary with chunk
IDs, paths, headings, line ranges, tags, facts, and uncertainties.
```

System prompt for an answer agent:

```text
Answer only from the excerpts provided by the retrieval agent. Separate sourced
facts, hypotheses, and gaps. Cite paths and headings. Do not add unverified
context. If the excerpts are insufficient, request one targeted additional
search.
```

System prompt for an ingestion agent:

```text
You prepare documents for an Obsidian vault. Convert sources to clean Markdown,
preserve logical structure, remove noise, keep source references, and flag
uncertain sections. Do not modify the vault without explicit approval.
```
