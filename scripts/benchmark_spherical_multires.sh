#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

timestamp="$(date +%Y%m%d_%H%M%S)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/benchmark_results/spherical_multires/$timestamp}"
TRIALS="${TRIALS:-3}"
OVERLAYS="${OVERLAYS:-swiss}"
CAMERA_ALT_M="${CAMERA_ALT_M:-90000}"
CAMERA_BACKOFF_M="${CAMERA_BACKOFF_M:-150000}"
WARMUP_SECONDS="${WARMUP_SECONDS:-8}"
DURATION_SECONDS="${DURATION_SECONDS:-20}"
READY_TIMEOUT_SECONDS="${READY_TIMEOUT_SECONDS:-120}"
PRESENT_MODE="${PRESENT_MODE:-auto_novsync}"
RUST_LOG_LEVEL="${RUST_LOG_LEVEL:-perf=info}"
CAPTURE_DIR="${CAPTURE_DIR:-}"
CAPTURE_FRAMES="${CAPTURE_FRAMES:-120,360,720}"
ENABLE_DEBUG_TOOLS="${ENABLE_DEBUG_TOOLS:-0}"
ENABLE_PERF_TITLE="${ENABLE_PERF_TITLE:-0}"
UPLOAD_BUDGET_MB="${UPLOAD_BUDGET_MB:-16}"

mkdir -p "$OUT_DIR"

echo "== Building release example =="
cargo build --release --example spherical_multires

BIN_PATH="$ROOT_DIR/target/release/examples/spherical_multires"
if [[ ! -x "$BIN_PATH" ]]; then
  echo "Benchmark binary missing: $BIN_PATH" >&2
  exit 1
fi

echo "== Running $TRIALS trial(s) =="
for trial in $(seq 1 "$TRIALS"); do
  base="$OUT_DIR/trial_${trial}"
  capture_dir_trial=""
  if [[ -n "$CAPTURE_DIR" ]]; then
    capture_dir_trial="$CAPTURE_DIR/trial_${trial}"
  fi
  echo "--- trial $trial/$TRIALS -> ${base}.json/.csv"
  # Release examples run outside `cargo run` need an explicit asset root.
  # Without this, Bevy may look for `assets/` relative to `target/release/examples`.
  RUST_LOG="$RUST_LOG_LEVEL" \
  BEVY_ASSET_ROOT="$ROOT_DIR" \
  MULTIRES_OVERLAYS="$OVERLAYS" \
  MULTIRES_CAMERA_ALT_M="$CAMERA_ALT_M" \
  MULTIRES_CAMERA_BACKOFF_M="$CAMERA_BACKOFF_M" \
  MULTIRES_PRESENT_MODE="$PRESENT_MODE" \
  MULTIRES_ENABLE_DEBUG_TOOLS="$ENABLE_DEBUG_TOOLS" \
  MULTIRES_ENABLE_PERF_TITLE="$ENABLE_PERF_TITLE" \
  MULTIRES_UPLOAD_BUDGET_MB="$UPLOAD_BUDGET_MB" \
  MULTIRES_CAPTURE_DIR="$capture_dir_trial" \
  MULTIRES_CAPTURE_FRAMES="$CAPTURE_FRAMES" \
  MULTIRES_BENCHMARK_READY_TIMEOUT_SECONDS="$READY_TIMEOUT_SECONDS" \
  MULTIRES_BENCHMARK_WARMUP_SECONDS="$WARMUP_SECONDS" \
  MULTIRES_BENCHMARK_DURATION_SECONDS="$DURATION_SECONDS" \
  MULTIRES_BENCHMARK_OUTPUT="$base" \
  "$BIN_PATH" || {
    echo "Trial $trial failed. Ensure a GPU/graphics context is available and terrain assets exist." >&2
    exit 1
  }
done

summary_csv="$OUT_DIR/summary.csv"
summary_txt="$OUT_DIR/summary.txt"

echo "== Aggregating results =="
{
  echo "trial,debug_tools_enabled,perf_title_enabled,ready_wait_s,ready_atlas_count,ready_loaded_atlas_count,ready_loaded_tile_total,fps_mean,frame_ms_mean,frame_ms_p95,latency_estimate_ms,sample_count"
  for csv in "$OUT_DIR"/trial_*.csv; do
    trial_name="$(basename "$csv" .csv)"
    awk -F, -v trial="$trial_name" 'NR==2 {printf "%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n", trial, $5, $6, $10, $11, $12, $13, $14, $15, $19, $22, $9}' "$csv"
  done
} > "$summary_csv"

awk -F, '
NR==1 {next}
{
  n++;
  ready_wait+=$4; fps+=$8; frame_mean+=$9; p95+=$10; latency+=$11;
  if ($10 > p95_max || n == 1) p95_max=$10;
  if ($9 < frame_best || n == 1) frame_best=$9;
}
END {
  if (n == 0) {
    print "No benchmark rows found.";
    exit 1;
  }
  printf "trials=%d\n", n;
  printf "ready_wait_s_avg=%.6f\n", ready_wait / n;
  printf "fps_mean_avg=%.6f\n", fps / n;
  printf "frame_ms_mean_avg=%.6f\n", frame_mean / n;
  printf "frame_ms_p95_avg=%.6f\n", p95 / n;
  printf "latency_estimate_ms_avg=%.6f\n", latency / n;
  printf "frame_ms_p95_worst=%.6f\n", p95_max;
  printf "frame_ms_mean_best=%.6f\n", frame_best;
}' "$summary_csv" > "$summary_txt"

echo "Benchmark artifacts:"
echo "  $OUT_DIR"
echo "  $summary_csv"
echo "  $summary_txt"
