# Benchmarks

`obsidian-kb` stays intentionally simple: vector search is brute-force over the
stored embedding table. These benchmarks make that limit explicit by separating
model startup, SQLite loading, vector decoding, cosine scoring, BM25, graph
expansion, and output hydration.

## Reproduce

Run the synthetic vault-size suite from the repository root:

```bash
tools/benchmark_vault_sizes.sh --chunks 1000,10000,50000 --runs 2
tools/benchmark_vault_sizes.sh --chunks 250000 --runs 3
```

By default the runner indexes generated Markdown with `--no-embeddings`, then
seeds deterministic 384-dimensional embeddings with
`examples/seed_synthetic_embeddings.rs`. This keeps the size sweep focused on
SQLite load, vector decode, and brute-force cosine scoring. To measure local
FastEmbed passage throughput instead, run:

```bash
tools/benchmark_vault_sizes.sh --chunks 1000,10000,50000 --runs 2 --full-embeddings
```

For the CI-style guardrail that isolates SQLite embedding load, vector decode,
and brute-force scoring without starting HTTP/MCP servers:

```bash
tools/benchmark_vault_sizes.sh --chunks 1000,10000 --runs 3 --vector-only
tools/check_benchmark_budget.sh --budgets benchmarks/budgets.json \
  "${TMPDIR:-/tmp}/obsidian-kb-vault-size-bench" 1000,10000
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

## Synthetic Corpus

Synthetic vaults are generated at benchmark time by
`examples/generate_synthetic_vault.rs`. The generator is deterministic and
reviewable in code, so benchmark corpora do not need compressed archives in the
repository.

The corpus shape is intentionally simple:

- exactly one `## Synthetic chunk NNNNN` heading per measured chunk, with no
  body text before the first measured heading;
- 100 chunks per note by default, which keeps note counts realistic enough to
  exercise parsing, chunking, SQLite replacement, and Tantivy indexing;
- deterministic repeated vocabulary across BM25, vector, graph, SQLite,
  Tantivy, MCP, and local retrieval topics;
- optional sparse wikilinks between notes, enabled by default, so graph parsing
  is exercised without turning graph expansion into the benchmark bottleneck;
- deterministic unit vectors seeded from chunk IDs and sized to the configured
  embedding model dimensions when `--full-embeddings` is not used.

The deterministic embeddings are not intended to simulate semantic quality.
They are there to make the size sweep cheap and stable enough to expose storage
load, vector decode, cache behavior, and brute-force cosine scoring. Use
`--full-embeddings` only when the benchmark target is local model throughput.

## Published Run

Environment: `obsidian-kb 0.0.0-snapshot`, release build, Darwin 25.5.0 arm64,
2026-05-23. Synthetic embeddings were used, so the indexing table excludes local
FastEmbed passage generation. The 1k, 10k, and 50k runs used two requests per
transport; the 250k run used three requests per transport to check that warm
cache behavior stayed stable after the first warm request.

### Indexing

| Chunks | Total ms | Load vault ms | SQLite replace ms | Tantivy rebuild ms |
| ---: | ---: | ---: | ---: | ---: |
| 1,000 | 240.9 | 8.0 | 6.8 | 210.6 |
| 10,000 | 438.4 | 51.3 | 62.9 | 279.4 |
| 50,000 | 1,741.5 | 230.6 | 393.6 | 904.5 |
| 250,000 | 9,701.1 | 1,260.2 | 2,702.7 | 4,631.4 |

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
| 250,000 | CLI | process-cold | 1,375.7 / 1,280.5 / 1,193.4 | 924.1 / 880.7 / 832.1 | 210.7 / 226.9 / 201.8 | 31.3 / 36.4 / 27.9 | 22.3 / 20.5 / 20.2 |
| 250,000 | HTTP REST | cold / warm / warm | 1,271.4 / 47.7 / 44.5 | 933.7 / 0.0 / 0.0 | 255.4 / 0.0 / 0.0 | 33.1 / 0.0 / 0.0 | 25.2 / 25.4 / 24.3 |
| 250,000 | MCP HTTP | cold / warm / warm | 1,144.5 / 47.8 / 43.3 | 846.1 / 0.0 / 0.0 | 218.3 / 0.0 / 0.0 | 35.9 / 0.0 / 0.0 | 21.0 / 20.9 / 23.4 |

The current brute-force implementation is still cheap at 250k synthetic chunks
when served from a warm process: warm HTTP/MCP search stayed under 50 ms, with
cosine scoring around 21-25 ms. Cold requests were dominated by local FastEmbed
model initialization plus SQLite embedding load/decode, not by brute-force
scoring alone. CLI search repeats model startup and embedding load on each
invocation, while `serve`/MCP keeps the model and decoded embeddings warm.

The 250k run does not yet justify moving retrieval to `sqlite-vec` by itself.
The more relevant trigger would be frequent cold searches, memory pressure from
keeping decoded embeddings warm, or target vault sizes closer to 500k-1M chunks.

## CI Guardrail

The CI guardrail lives in `.github/workflows/benchmarks.yml`. It is intentionally
coarse: GitHub-hosted runners are noisy, and local FastEmbed model initialization
can fluctuate enough to make tight thresholds brittle.

The workflow:

- runs on benchmark-relevant pull requests with 1k and 10k synthetic chunks;
- runs weekly with 1k, 10k, and 50k chunks;
- can be manually dispatched with any comma-separated chunk list, including
  250k for stress checks;
- uses `--vector-only`, deterministic embeddings, and no HTTP/MCP server so it
  avoids network binding and query model initialization in CI;
- validates summaries with `tools/check_benchmark_budget.sh`, using wide
  thresholds from `benchmarks/budgets.json` for indexing, warm vector total
  time, and `vector_score_embeddings_ms`;
- writes a Markdown comparison table to `GITHUB_STEP_SUMMARY`, so each run shows
  actual values, budgets, and pass/fail status directly in the GitHub Actions
  summary;
- uploads the generated JSONL and summary files as workflow artifacts.

The guardrail should fail only on broad regressions in stable phases, such as
indexing becoming several times slower, warm vector search exceeding a generous
threshold, or `vector_score_embeddings_ms` growing non-linearly for the same
chunk count. It should not be interpreted as a precise performance scoreboard.
For hosted GitHub runners, the checked-in budgets are formula-based and
intentionally loose:

- `index_total_ms`: `30000 + chunks * 0.1`
- `warm_vector_total_ms`: `500 + chunks * 0.01`
- `warm_vector_score_ms`: `100 + chunks * 0.005`

Specific chunk sizes can be tightened later with explicit overrides in
`benchmarks/budgets.json` once there is enough artifact history from `main`.

Compressed vault archives are only worth considering for a separate realistic
corpus benchmark. If that becomes useful, store the archive as a GitHub Release
asset or workflow cache input rather than adding it to normal Git history.

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
