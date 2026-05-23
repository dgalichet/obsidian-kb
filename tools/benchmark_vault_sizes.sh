#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: tools/benchmark_vault_sizes.sh [options]

Generate synthetic Obsidian vaults and run public size benchmarks.

Options:
  --chunks LIST       Chunk counts to generate, comma or space separated.
                      Default: "1000 10000 50000"
  --workdir DIR       Output directory. Default: /tmp/obsidian-kb-vault-size-bench
  --binary PATH       obsidian-kb binary. Default: target/release/obsidian-kb
  --runs N            Requests per transport after indexing. Default: 2
  --full-embeddings   Build local FastEmbed passage embeddings instead of
                      deterministic synthetic embeddings.
  --vector-only       Run direct synthetic vector benchmarks only. This skips
                      CLI/HTTP/MCP query embedding and is intended for CI.
  --no-build          Do not run cargo build --release before benchmarking.
  -h, --help          Show this help.

The default path uses deterministic synthetic embeddings so the 50k chunk
search benchmark remains fast and isolates SQLite load, vector decode, and
brute-force cosine scoring. Use --full-embeddings when measuring local model
throughput is the benchmark goal.
EOF
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

chunks_list="1000 10000 50000"
workdir="${TMPDIR:-/tmp}/obsidian-kb-vault-size-bench"
binary="target/release/obsidian-kb"
runs=2
full_embeddings=0
vector_only=0
build_release=1
query="synthetic local retrieval graph benchmark memory agent"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --chunks)
      chunks_list="${2//,/ }"
      shift 2
      ;;
    --workdir)
      workdir="$2"
      shift 2
      ;;
    --binary)
      binary="$2"
      shift 2
      ;;
    --runs)
      runs="$2"
      shift 2
      ;;
    --full-embeddings)
      full_embeddings=1
      shift
      ;;
    --vector-only)
      vector_only=1
      shift
      ;;
    --no-build)
      build_release=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

need cargo
need jq
if [ "$vector_only" -eq 0 ]; then
  need curl
fi

if [ "$build_release" -eq 1 ]; then
  cargo build --release
fi

if [ ! -x "$binary" ]; then
  echo "binary is not executable: $binary" >&2
  exit 1
fi

mkdir -p "$workdir"

write_config() {
  local vault="$1"
  cat > "$vault/.obsidian-kb.toml" <<EOF
[vault]
path = "$vault"
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
exclude_headings = []
remove_diacritics = true

[index.properties]
enabled = true
filter_keys = ["*"]
ignored_keys = ["cssclasses", "template", "id", "uuid", "publish", "dg-*"]
max_value_chars = 200

[index.pdf]
enabled = false
max_file_size_mb = 50

[search]
default_mode = "hybrid"
bm25_candidates = 80
vector_candidates = 80
final_top_k = 10
rrf_k = 60.0
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

[benchmark]
enabled = true
log_path = ".obsidian-kb/benchmarks.jsonl"
include_query = false

[mcp]
idle_unload_seconds = 0
preload_embedder = false

[serve]
cors_allowed_origins = ["app://obsidian.md"]

[doctor.unresolved_links]
allow_forward_links = false
ignore_targets = []
ignore_globs = []
EOF
}

start_server() {
  local config="$1"
  local log="$2"
  "$binary" --config "$config" serve --port 0 > "$log" 2>&1 &
  server_pid=$!
  server_url=""
  for _ in $(seq 1 200); do
    if grep -q "obsidian-kb serve listening on " "$log"; then
      server_url="$(sed -n 's/^obsidian-kb serve listening on //p' "$log" | tail -n 1)"
      break
    fi
    sleep 0.1
  done
  if [ -z "$server_url" ]; then
    echo "server did not start; see $log" >&2
    kill "$server_pid" >/dev/null 2>&1 || true
    exit 1
  fi
}

stop_server() {
  if [ -n "${server_url:-}" ]; then
    curl -fsS -X POST "$server_url/shutdown" >/dev/null 2>&1 || true
  fi
  if [ -n "${server_pid:-}" ]; then
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
  server_url=""
  server_pid=""
}

run_http_searches() {
  local body
  body="$(jq -nc --arg query "$query" '{query:$query, mode:"hybrid", expand_graph:true, top:8}')"
  for _ in $(seq 1 "$runs"); do
    curl -fsS -H "content-type: application/json" -d "$body" "$server_url/search" >/dev/null
  done
}

run_mcp_http_searches() {
  local body
  body="$(jq -nc --arg query "$query" '{
    jsonrpc:"2.0",
    id:1,
    method:"tools/call",
    params:{
      name:"search",
      arguments:{query:$query, mode:"hybrid", expand_graph:true, top:8}
    }
  }')"
  for _ in $(seq 1 "$runs"); do
    curl -fsS -H "content-type: application/json" -d "$body" "$server_url/mcp" >/dev/null
  done
}

summarize_log() {
  local log_path="$1"
  local summary_path="$2"
  jq -R -s '
    def valid:
      split("\n")
      | map(select(length > 0) | (fromjson?))
      | map(select(. != null));
    def row:
      {
        command,
        transport,
        total_ms,
        chunks,
        vector_embedding_count,
        vector_embeddings_cached,
        vector_embedder_cached,
        phases
      };
    valid | map(row)
  ' "$log_path" > "$summary_path"
}

for chunks in $chunks_list; do
  vault="$workdir/vault-${chunks}"
  config="$vault/.obsidian-kb.toml"
  log_path="$vault/.obsidian-kb/benchmarks.jsonl"
  outdir="$workdir/results-${chunks}"
  mkdir -p "$outdir"

  echo "== ${chunks} chunks =="
  cargo run --release --quiet --example generate_synthetic_vault -- \
    --out "$vault" \
    --chunks "$chunks" \
    --chunks-per-note 100 \
    --links sparse \
    --overwrite > "$outdir/generate-vault.json"
  write_config "$vault"

  if [ "$full_embeddings" -eq 1 ]; then
    "$binary" --config "$config" index --rebuild > "$outdir/index.txt"
  else
    "$binary" --config "$config" index --rebuild --no-embeddings > "$outdir/index.txt"
    cargo run --release --quiet --example seed_synthetic_embeddings -- --config "$config" \
      > "$outdir/seed-embeddings.txt"
  fi

  if [ "$vector_only" -eq 1 ]; then
    cargo run --release --quiet --example benchmark_synthetic_vectors -- \
      --config "$config" \
      --runs "$runs" \
      --limit 80 > "$outdir/synthetic-vector.txt"
  else
    for _ in $(seq 1 "$runs"); do
      "$binary" --config "$config" search "$query" --mode hybrid --expand-graph --top 8 --json \
        > /dev/null
    done

    start_server "$config" "$outdir/http-rest.log"
    run_http_searches
    stop_server

    start_server "$config" "$outdir/mcp-http.log"
    run_mcp_http_searches
    stop_server
  fi

  summarize_log "$log_path" "$outdir/summary.json"
  cp "$log_path" "$outdir/benchmarks.jsonl"
  echo "wrote $outdir"
done

echo "done: $workdir"
