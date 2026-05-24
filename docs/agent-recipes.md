# Agent Recipes

This page turns the core agent rule into copy-paste integrations:

```text
Search first. Read second.
```

Use these recipes when an LLM agent, MCP client, editor agent, or shell script
needs to answer questions from an Obsidian vault indexed by `obsidian-kb`.

## Before You Copy

Replace these placeholders in every recipe:

- `/path/to/ObsidianVault` with the absolute path to the vault.
- `/path/to/ObsidianVault/.obsidian-kb.toml` with the absolute path to the
  `obsidian-kb` config.
- `obsidian-kb` with the full binary path when a GUI client cannot find it, for
  example `/opt/homebrew/bin/obsidian-kb` or
  `${HOME}/.cargo/bin/obsidian-kb`.

Prepare the vault once:

```bash
cd /path/to/ObsidianVault
obsidian-kb init --vault .
obsidian-kb index
obsidian-kb doctor
```

External agents may send the returned excerpts to their model provider. The
index, embeddings, MCP server, and retrieval work stay local inside
`obsidian-kb`.

## System Prompt

Paste this into a system prompt, project instruction file, or agent rule:

```text
You answer questions about an Obsidian vault through obsidian-kb.

Search first. Read second. Never inspect the whole vault or bulk-read notes.
For every content question, start with obsidian-kb search or the MCP search
tool. Use hybrid search with graph expansion for conceptual questions, BM25 for
exact names, commands, error messages, APIs, classes, acronyms, and file names,
and vector search for vague semantic questions. Prefer compact JSON search
results with included text before calling show. Read only selected chunks. Cite
note paths, headings, and line ranges. Do not modify vault notes unless the
user explicitly asks.
```

Shorter variant:

```text
For Obsidian vault questions: search first, read second. Use obsidian-kb search
or MCP search before reading files. Use bm25 for exact strings, vector for vague
ideas, and hybrid with expand_graph for conceptual questions. Read only selected
chunks with show. Cite path, heading, and line range. Never bulk-read the vault.
```

## Common MCP Calls

Most MCP clients hide the raw JSON, but these argument shapes are useful when a
client asks what to send to the tool.

Conceptual question:

```json
{
  "query": "Obsidian as a local RAG knowledge base",
  "mode": "hybrid",
  "expand_graph": true,
  "top": 5,
  "include_text": true,
  "max_chars": 1200
}
```

Exact term, command, API, or error:

```json
{
  "query": "Service Connect TLS App Mesh",
  "mode": "bm25",
  "top": 5,
  "include_text": true,
  "max_chars": 1200
}
```

Vague semantic query:

```json
{
  "query": "ways to avoid overloading an agent context",
  "mode": "vector",
  "top": 5,
  "include_text": true,
  "max_chars": 1200
}
```

Read one selected chunk:

```json
{
  "chunk_id": "paste-result-chunk-id-here"
}
```

Read several selected chunks in one call:

```json
{
  "chunk_ids": [
    "paste-first-result-chunk-id-here",
    "paste-second-result-chunk-id-here"
  ]
}
```

## Claude Desktop MCP

Open Claude Desktop settings, use the Developer section to edit the MCP config,
then add `obsidian-kb` under `mcpServers`.

macOS config path:

```text
~/Library/Application Support/Claude/claude_desktop_config.json
```

Windows config path:

```text
%APPDATA%\Claude\claude_desktop_config.json
```

Configuration:

```json
{
  "mcpServers": {
    "obsidian-kb": {
      "command": "obsidian-kb",
      "args": [
        "--config",
        "/path/to/ObsidianVault/.obsidian-kb.toml",
        "mcp"
      ]
    }
  }
}
```

If Claude Desktop logs `ENOENT` or cannot start the server, replace
`"command": "obsidian-kb"` with the absolute binary path:

```json
{
  "mcpServers": {
    "obsidian-kb": {
      "command": "/opt/homebrew/bin/obsidian-kb",
      "args": [
        "--config",
        "/path/to/ObsidianVault/.obsidian-kb.toml",
        "mcp"
      ]
    }
  }
}
```

Restart Claude Desktop after editing the config. Then ask:

```text
Use obsidian-kb to search my vault for "hybrid retrieval". Return only the
source paths and headings you found.
```

The useful first tool call should be `search`, not filesystem reads.

## Codex Or CLI Agents

For Codex, terminal agents, or any agent that can run shell commands, add this
to the repository or vault `AGENTS.md`:

```markdown
## Obsidian vault retrieval

- Never read the whole vault.
- Always run `obsidian-kb search` before reading note files.
- Use `--mode hybrid --expand-graph` for conceptual questions.
- Use `--mode bm25` for exact names, commands, errors, APIs, classes,
  acronyms, and file names.
- Use `--mode vector` for vague semantic questions.
- Prefer `--json --include-text --max-chars 1200` for first-pass context.
- Use `obsidian-kb show <chunk-id> --json` only for selected chunks; pass
  several chunk ids in one command when reading multiple selected chunks.
- Cite note path, heading, and line range.
- Do not modify notes unless the user explicitly asks.
```

Give the agent command templates:

```bash
obsidian-kb --config /path/to/ObsidianVault/.obsidian-kb.toml \
  search "conceptual question" \
  --mode hybrid \
  --expand-graph \
  --top 5 \
  --json \
  --include-text \
  --max-chars 1200

obsidian-kb --config /path/to/ObsidianVault/.obsidian-kb.toml \
  search "ExactNameOrError" \
  --mode bm25 \
  --top 5 \
  --json \
  --include-text \
  --max-chars 1200

obsidian-kb --config /path/to/ObsidianVault/.obsidian-kb.toml \
  show <chunk-id> \
  --json

obsidian-kb --config /path/to/ObsidianVault/.obsidian-kb.toml \
  show <chunk-id> <chunk-id> \
  --json
```

Prompt for a one-shot terminal agent:

```text
Answer this vault question using obsidian-kb. Start with search, read only
selected chunks, and cite path, heading, and line range:

<question>
```

## Cursor MCP

Use a project config when one repository should know about one vault:

```text
.cursor/mcp.json
```

Use a global config when the vault should be available in every Cursor project:

```text
~/.cursor/mcp.json
```

Configuration:

```json
{
  "mcpServers": {
    "obsidian-kb": {
      "type": "stdio",
      "command": "obsidian-kb",
      "args": [
        "--config",
        "/path/to/ObsidianVault/.obsidian-kb.toml",
        "mcp"
      ]
    }
  }
}
```

Add a Cursor rule beside the MCP config:

```text
.cursor/rules/obsidian-kb.mdc
```

Rule:

```markdown
---
description: Use obsidian-kb for Obsidian vault retrieval
alwaysApply: false
---

When the user asks about the Obsidian vault, use the obsidian-kb MCP tools.
Search first, read second. Start with `search`; use `show` only for selected
chunks, and use `chunk_ids` when reading several selected chunks. Use
`mode: "bm25"` for exact names, commands, errors, APIs, classes, acronyms, and
file names. Use `mode: "vector"` for vague semantic questions. Use
`mode: "hybrid"` with `expand_graph: true` for conceptual questions. Cite path,
heading, and line range. Do not bulk-read the vault or modify notes unless the
user explicitly asks.
```

After Cursor reloads the MCP config, ask:

```text
Use obsidian-kb to answer: what notes discuss context overload? Cite paths and
headings.
```

## Continue MCP

Add an MCP server block to Continue's `config.yaml`:

```yaml
mcpServers:
  - name: obsidian-kb
    type: stdio
    command: obsidian-kb
    args:
      - --config
      - /path/to/ObsidianVault/.obsidian-kb.toml
      - mcp
```

If you already maintain JSON MCP configs for other clients, Continue can also
load JSON configs from:

```text
.continue/mcpServers/
```

For example:

```text
.continue/mcpServers/obsidian-kb.json
```

```json
{
  "mcpServers": {
    "obsidian-kb": {
      "command": "obsidian-kb",
      "args": [
        "--config",
        "/path/to/ObsidianVault/.obsidian-kb.toml",
        "mcp"
      ]
    }
  }
}
```

Add this rule to Continue:

```text
For Obsidian vault questions, use the obsidian-kb MCP tools. Search first, read
second. Use search before show; pass chunk_ids when reading several selected
chunks. Use bm25 for exact terms, vector for vague ideas, and hybrid with
expand_graph for conceptual questions. Cite path, heading, and line range. Do
not bulk-read or modify the vault unless asked.
```

## Shell Scripts

These scripts are useful for agents that can call small tools but should not
remember long command lines. Put them in a directory on `PATH`, then set
`OBSIDIAN_KB_CONFIG`.

```bash
export OBSIDIAN_KB_CONFIG=/path/to/ObsidianVault/.obsidian-kb.toml
```

`kb-search` for conceptual questions:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "Usage: kb-search QUERY" >&2
  exit 2
fi

: "${OBSIDIAN_KB_CONFIG:?Set OBSIDIAN_KB_CONFIG=/path/to/.obsidian-kb.toml}"

obsidian-kb --config "$OBSIDIAN_KB_CONFIG" \
  search "$*" \
  --mode hybrid \
  --expand-graph \
  --top "${OBSIDIAN_KB_TOP:-5}" \
  --json \
  --include-text \
  --max-chars "${OBSIDIAN_KB_MAX_CHARS:-1200}"
```

`kb-exact` for names, commands, APIs, classes, errors, acronyms, and file names:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "Usage: kb-exact QUERY" >&2
  exit 2
fi

: "${OBSIDIAN_KB_CONFIG:?Set OBSIDIAN_KB_CONFIG=/path/to/.obsidian-kb.toml}"

obsidian-kb --config "$OBSIDIAN_KB_CONFIG" \
  search "$*" \
  --mode bm25 \
  --top "${OBSIDIAN_KB_TOP:-5}" \
  --json \
  --include-text \
  --max-chars "${OBSIDIAN_KB_MAX_CHARS:-1200}"
```

`kb-show` for one or more selected chunks:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "Usage: kb-show CHUNK_ID [CHUNK_ID...]" >&2
  exit 2
fi

: "${OBSIDIAN_KB_CONFIG:?Set OBSIDIAN_KB_CONFIG=/path/to/.obsidian-kb.toml}"

obsidian-kb --config "$OBSIDIAN_KB_CONFIG" show "$@" --json
```

The expected workflow is:

```bash
kb-search "how should agents avoid context overload?"
kb-show <chunk-id-from-search>
kb-show <chunk-id-from-search> <another-chunk-id-from-search>
```

Do not use these scripts as a reason to read every matching file. They exist to
keep retrieval narrow.

## Troubleshooting

- Server does not start: use the absolute path to `obsidian-kb`.
- Client shows no MCP tools: restart the client and validate JSON or YAML
  syntax.
- Search misses recent notes: run `obsidian-kb index`, then retry the search.
- Vector search is slow on the first call: call the MCP `warmup` tool or set
  `mcp.preload_embedder = true`.
- Results are noisy: reformulate once, switch mode, or add tag/property filters
  before increasing `top`.
- Do not run `index --rebuild` before every answer. Use a rebuild only when
  config, chunking, embedding model, or index corruption requires it.

## Client References

- MCP local server setup:
  <https://modelcontextprotocol.io/docs/develop/connect-local-servers>
- Cursor MCP configuration:
  <https://docs.cursor.com/context/model-context-protocol>
- Continue MCP configuration:
  <https://docs.continue.dev/customize/deep-dives/mcp>
