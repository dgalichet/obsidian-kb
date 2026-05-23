#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: tools/check_benchmark_budget.sh [options] WORKDIR [CHUNKS]

Validate synthetic benchmark summaries against broad regression budgets.

Options:
  --budgets PATH  Budget JSON file. Default: benchmarks/budgets.json
  --summary PATH  Markdown summary output. Default: $GITHUB_STEP_SUMMARY when set
  -h, --help      Show this help.

Arguments:
  WORKDIR  Directory produced by tools/benchmark_vault_sizes.sh.
  CHUNKS   Optional comma or space separated chunk counts. When omitted,
           every WORKDIR/results-* directory is checked.

The budgets are intentionally generous. They are meant to catch order-of-
magnitude regressions in indexing, warm vector search, or brute-force cosine
scoring, not to compare noisy CI runner timings precisely.
EOF
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

budgets_path="benchmarks/budgets.json"
summary_path="${GITHUB_STEP_SUMMARY:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --budgets)
      budgets_path="$2"
      shift 2
      ;;
    --summary)
      summary_path="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
    *)
      break
      ;;
  esac
done

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  usage >&2
  exit 1
fi

need jq

workdir="$1"
chunks_list="${2:-}"

if [ ! -d "$workdir" ]; then
  echo "benchmark workdir does not exist: $workdir" >&2
  exit 1
fi

if [ ! -s "$budgets_path" ]; then
  echo "benchmark budget file does not exist: $budgets_path" >&2
  exit 1
fi

if [ -z "$chunks_list" ]; then
  chunks_list="$(
    find "$workdir" -maxdepth 1 -type d -name 'results-*' -print \
      | sed 's/^.*results-//' \
      | sort -n
  )"
fi

chunks_list="${chunks_list//,/ }"
if [ -z "$chunks_list" ]; then
  echo "no benchmark result directories found under $workdir" >&2
  exit 1
fi

summary_rows=""
failed=0

evaluate_summary() {
  local chunks="$1"
  local summary="$2"

  jq -n \
    --slurpfile rows "$summary" \
    --slurpfile budgets "$budgets_path" \
    --arg chunks "$chunks" '
      def chunk_count: ($chunks | tonumber);
      def phase($row; $name): (($row.phases[$name] // 0) | tonumber);
      def max_or_null: if length > 0 then max else null end;
      def metric_budget($metric):
        (
          $budgets[0].overrides[$chunks][$metric]
          // (
            ($budgets[0].metrics[$metric].base_ms // 0)
            + (($budgets[0].metrics[$metric].per_chunk_ms // 0) * chunk_count)
          )
        );

      ($rows[0]) as $records
      | ($records | map(select(.command == "index"))) as $index
      | ($records | map(select(.command == "synthetic_vector_search"))) as $vector
      | ($vector | map(select(.vector_embeddings_cached == false))) as $cold
      | ($vector | map(select(.vector_embeddings_cached == true))) as $warm
      | {
          chunks: chunk_count,
          budgets: {
            index_total_ms: metric_budget("index_total_ms"),
            warm_vector_total_ms: metric_budget("warm_vector_total_ms"),
            warm_vector_score_ms: metric_budget("warm_vector_score_ms")
          },
          metrics: {
            indexed_chunks: ($index[0].chunks // null),
            index_total_ms: ($index[0].total_ms // null),
            vector_runs: ($vector | length),
            cold_runs: ($cold | length),
            warm_runs: ($warm | length),
            warm_vector_total_ms: ($warm | map(.total_ms) | max_or_null),
            warm_vector_score_ms: (
              $warm
              | map(phase(.; "vector_score_embeddings_ms"))
              | max_or_null
            )
          }
        }
      | .checks = {
          has_index: (.metrics.indexed_chunks != null),
          chunks_match: (.metrics.indexed_chunks == .chunks),
          index_total_ms: (
            .metrics.index_total_ms != null
            and .metrics.index_total_ms <= .budgets.index_total_ms
          ),
          has_vector_runs: (.metrics.vector_runs >= 2),
          has_cold_run: (.metrics.cold_runs >= 1),
          has_warm_run: (.metrics.warm_runs >= 1),
          warm_vector_total_ms: (
            .metrics.warm_vector_total_ms != null
            and .metrics.warm_vector_total_ms <= .budgets.warm_vector_total_ms
          ),
          warm_vector_score_ms: (
            .metrics.warm_vector_score_ms != null
            and .metrics.warm_vector_score_ms <= .budgets.warm_vector_score_ms
          )
        }
      | .ok = ([.checks[]] | all)
    '
}

format_rows() {
  jq -r '
    def fmt:
      if . == null then
        "n/a"
      elif type == "number" then
        (. * 10 | round / 10 | tostring)
      else
        tostring
      end;
    def status($ok): if $ok then "ok" else "fail" end;
    [
      [.chunks, "indexed chunks", (.metrics.indexed_chunks | fmt), (.chunks | tostring), status(.checks.chunks_match)],
      [.chunks, "index total ms", (.metrics.index_total_ms | fmt), (.budgets.index_total_ms | fmt), status(.checks.index_total_ms)],
      [.chunks, "vector runs", (.metrics.vector_runs | tostring), ">= 2", status(.checks.has_vector_runs)],
      [.chunks, "cold vector runs", (.metrics.cold_runs | tostring), ">= 1", status(.checks.has_cold_run)],
      [.chunks, "warm vector runs", (.metrics.warm_runs | tostring), ">= 1", status(.checks.has_warm_run)],
      [.chunks, "warm vector total ms", (.metrics.warm_vector_total_ms | fmt), (.budgets.warm_vector_total_ms | fmt), status(.checks.warm_vector_total_ms)],
      [.chunks, "warm vector score ms", (.metrics.warm_vector_score_ms | fmt), (.budgets.warm_vector_score_ms | fmt), status(.checks.warm_vector_score_ms)]
    ]
    | .[]
    | "| \(.[0]) | \(.[1]) | \(.[2]) | \(.[3]) | \(.[4]) |"
  '
}

for chunks in $chunks_list; do
  summary="$workdir/results-${chunks}/summary.json"
  if [ ! -s "$summary" ]; then
    echo "missing benchmark summary: $summary" >&2
    if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
      echo "::error title=Benchmark summary missing::${summary}"
    fi
    failed=1
    continue
  fi

  result="$(evaluate_summary "$chunks" "$summary")"
  summary_rows="${summary_rows}
$(printf '%s\n' "$result" | format_rows)"

  if printf '%s\n' "$result" | jq -e '.ok' >/dev/null; then
    echo "ok: ${chunks} chunks within benchmark budget"
  else
    echo "benchmark budget failed for ${chunks} chunks" >&2
    printf '%s\n' "$result" | jq . >&2
    if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
      echo "::error title=Benchmark budget failed::${chunks} chunks exceeded benchmark budget"
    fi
    failed=1
  fi
done

if [ -n "$summary_path" ]; then
  {
    echo "## Synthetic Benchmark Budgets"
    echo
    echo "Budget file: \`$budgets_path\`"
    echo
    echo "| chunks | metric | actual | budget | status |"
    echo "| ---: | --- | ---: | ---: | --- |"
    printf '%s\n' "$summary_rows" | sed '/^$/d'
    echo
  } >> "$summary_path"
fi

exit "$failed"
