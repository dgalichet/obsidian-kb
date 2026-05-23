# Benchmarks

`obsidian-kb` stays intentionally simple: vector search is brute-force over the
stored embedding table. These benchmarks make that limit explicit by separating
model startup, SQLite loading, vector decoding, cosine scoring, BM25, graph
expansion, and output hydration.

## Reproduce

Run the synthetic vault-size suite from the repository root:

```bash
tools/benchmark_vault_sizes.sh --chunks 1000,10000,50000 --runs 2
```

By default the runner indexes generated Markdown with `--no-embeddings`, then
seeds deterministic 384-dimensional embeddings with
`examples/seed_synthetic_embeddings.rs`. This keeps the size sweep focused on
SQLite load, vector decode, and brute-force cosine scoring. To measure local
FastEmbed passage throughput instead, run:

```bash
tools/benchmark_vault_sizes.sh --chunks 1000,10000,50000 --runs 2 --full-embeddings
```

The runner records:

- CLI: `obsidian-kb search`; every run is process-cold for the vector model and
  decoded embedding cache.
- HTTP REST: `obsidian-kb serve` plus `POST /search`; first request is cold,
  second request is warm in the same process.
- MCP over HTTP: `obsidian-kb serve` plus `POST /mcp` `tools/call search`;
  first request is cold, second request is warm in the same process.

Raw machine-readable results for the published run are in
[`benchmarks/vault-size-2026-05-23.json`](../benchmarks/vault-size-2026-05-23.json).

## Published Run

Environment: `obsidian-kb 0.0.0-snapshot`, release build, Darwin 25.5.0 arm64,
2026-05-23. Synthetic embeddings were used, so the indexing table excludes local
FastEmbed passage generation.

### Indexing

| Chunks | Total ms | Load vault ms | SQLite replace ms | Tantivy rebuild ms |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 240.9 | 8.0 | 6.8 | 210.6 |
| 10,000 | 438.4 | 51.3 | 62.9 | 279.4 |
| 50,000 | 1,741.5 | 230.6 | 393.6 | 904.5 |

### Search

| Chunks | Transport | Cache | Total ms | Model init ms | Load ms | Decode ms | Score ms |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1,000 | CLI | process-cold | 946.6 / 888.7 | 871.1 / 813.4 | 1.1 / 1.1 | 0.1 / 0.2 | 0.3 / 0.2 |
| 1,000 | HTTP REST | cold / warm | 879.5 / 20.7 | 862.7 / 0.0 | 1.1 / 0.0 | 0.1 / 0.0 | 0.2 / 0.2 |
| 1,000 | MCP HTTP | cold / warm | 939.6 / 20.7 | 918.8 / 0.0 | 1.4 / 0.0 | 0.1 / 0.0 | 0.2 / 0.3 |
| 10,000 | CLI | process-cold | 893.8 / 941.9 | 807.0 / 852.3 | 8.8 / 9.6 | 0.8 / 0.9 | 1.1 / 5.4 |
| 10,000 | HTTP REST | cold / warm | 845.8 / 21.2 | 810.9 / 0.0 | 8.2 / 0.0 | 1.1 / 0.0 | 4.2 / 1.6 |
| 10,000 | MCP HTTP | cold / warm | 873.4 / 19.1 | 842.4 / 0.0 | 7.9 / 0.0 | 0.8 / 0.0 | 0.9 / 0.9 |
| 50,000 | CLI | process-cold | 963.3 / 966.6 | 823.4 / 781.9 | 40.1 / 42.4 | 11.3 / 12.4 | 4.0 / 4.0 |
| 50,000 | HTTP REST | cold / warm | 873.9 / 22.8 | 803.6 / 0.0 | 38.2 / 0.0 | 6.1 / 0.0 | 3.5 / 4.4 |
| 50,000 | MCP HTTP | cold / warm | 910.7 / 26.9 | 843.5 / 0.0 | 38.2 / 0.0 | 5.5 / 0.0 | 4.2 / 4.7 |

The current brute-force implementation is still cheap at 50k synthetic chunks:
warm HTTP/MCP search stays under 30 ms, with cosine scoring around 4-5 ms. Cold
requests are dominated by local FastEmbed model initialization, not brute-force
scoring. CLI search repeats that startup work on each invocation, while
`serve`/MCP keeps the model and decoded embeddings warm.

## Real Vault Sanity Check

The private working vault used during development had about 3.3k stored
embeddings in recent benchmark logs. Representative observations:

- CLI vector or hybrid searches were usually around 0.84-1.66 s, mostly from
  `vector_embedder_init_ms`; brute-force scoring was about 0.6-1.8 ms.
- Warm MCP hybrid searches were commonly 30-55 ms, with
  `vector_embeddings_cached = true`; brute-force scoring was usually below
  11 ms.
- Routine indexing on the working vault showed SQLite replacement around
  190-300 ms and Tantivy rebuild around 270-340 ms. When many embeddings were
  reused, `embeddings_rebuild_ms` could be near 20 ms; when new chunks needed
  embeddings, that phase dominated the run.
