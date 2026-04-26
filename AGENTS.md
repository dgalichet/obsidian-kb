# AGENTS.md

## Project purpose

This repository contains `obsidian-kb`, a local-first Rust CLI for hybrid retrieval over an Obsidian vault.

## Rules for Codex

- For questions about the Obsidian vault, never read the entire vault.
- Always use `obsidian-kb search` first.
- Use `--mode hybrid --expand-graph` for conceptual questions.
- Use `--mode bm25` for exact names, commands, error messages, APIs, classes, acronyms, or file names.
- Use `--mode vector` for vague conceptual questions.
- Read only the top relevant chunks returned by the tool.
- Cite file paths and headings when summarizing.
- Do not modify the user's vault unless explicitly asked.
- Do not add hosted LLM API calls inside `obsidian-kb`.
- Keep the tool local-first.
- Prefer small, testable Rust modules.
- Run tests before reporting completion.

## Build commands

- `cargo build`
- `cargo test`
- `cargo clippy --all-targets --all-features`
- `cargo fmt --all --check`

## Development expectations

- Keep public functions typed and documented.
- Prefer clear errors over silent failure.
- Avoid unnecessary abstractions.
- Keep indexing idempotent.
- Keep search explainable.
