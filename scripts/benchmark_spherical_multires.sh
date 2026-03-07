#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

timestamp="$(date +%Y%m%d_%H%M%S)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/benchmark_results/spherical_multires/$timestamp}"
EXAMPLE_FEATURES="${EXAMPLE_FEATURES:-}"
TRIALS="${TRIALS:-3}"
OVERLAYS="${OVERLAYS:-swiss}"
CAMERA_ALT_M="${CAMERA_ALT_M:-90000}"
CAMERA_BACKOFF_M="${CAMERA_BACKOFF_M:-150000}"
BENCHMARK_SWEEP_DEG="${BENCHMARK_SWEEP_DEG:-8}"
BENCHMARK_SWEEP_PERIOD_SECONDS="${BENCHMARK_SWEEP_PERIOD_SECONDS:-40}"
WARMUP_SECONDS="${WARMUP_SECONDS:-8}"
DURATION_SECONDS="${DURATION_SECONDS:-20}"
READY_TIMEOUT_SECONDS="${READY_TIMEOUT_SECONDS:-120}"
PRESENT_MODE="${PRESENT_MODE:-auto_novsync}"
RUST_LOG_LEVEL="${RUST_LOG_LEVEL:-perf=info}"
CAPTURE_DIR="${CAPTURE_DIR:-}"
CAPTURE_FRAMES="${CAPTURE_FRAMES:-120,360,720}"
ENABLE_DEBUG_TOOLS="${ENABLE_DEBUG_TOOLS:-0}"
ENABLE_PERF_TITLE="${ENABLE_PERF_TITLE:-0}"
ENABLE_DRONE="${ENABLE_DRONE:-0}"
UPLOAD_BUDGET_MB="${UPLOAD_BUDGET_MB:-24}"
MSAA_SAMPLES="${MSAA_SAMPLES:-4}"
METAL_CAPTURE_ENABLED="${METAL_CAPTURE_ENABLED:-0}"
METAL_CAPTURE_FRAME="${METAL_CAPTURE_FRAME:-}"
METAL_CAPTURE_DIR="${METAL_CAPTURE_DIR:-}"
SCENARIO_NAME="${SCENARIO_NAME:-${OVERLAYS}_${PRESENT_MODE}_sweep${BENCHMARK_SWEEP_DEG}_drone${ENABLE_DRONE}}"

mkdir -p "$OUT_DIR"

if [[ -n "$METAL_CAPTURE_FRAME" && -n "$CAPTURE_DIR" ]]; then
  IFS=',' read -r -a capture_frames_requested <<< "$CAPTURE_FRAMES"
  for frame in "${capture_frames_requested[@]}"; do
    frame_trimmed="$(echo "$frame" | xargs)"
    [[ -n "$frame_trimmed" ]] || continue
    if [[ "$frame_trimmed" == "$METAL_CAPTURE_FRAME" ]]; then
      echo "CAPTURE_FRAMES and METAL_CAPTURE_FRAME must not target the same frame ($METAL_CAPTURE_FRAME)." >&2
      exit 1
    fi
  done
fi

echo "== Building release example =="
cargo_build_args=(build --release --example spherical_multires)
if [[ -n "$EXAMPLE_FEATURES" ]]; then
  cargo_build_args+=(--features "$EXAMPLE_FEATURES")
fi
cargo "${cargo_build_args[@]}"

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
  MULTIRES_BENCHMARK_SCENARIO="$SCENARIO_NAME" \
  MULTIRES_BENCHMARK_SWEEP_DEG="$BENCHMARK_SWEEP_DEG" \
  MULTIRES_BENCHMARK_SWEEP_PERIOD_SECONDS="$BENCHMARK_SWEEP_PERIOD_SECONDS" \
  MULTIRES_ENABLE_DEBUG_TOOLS="$ENABLE_DEBUG_TOOLS" \
  MULTIRES_ENABLE_PERF_TITLE="$ENABLE_PERF_TITLE" \
  MULTIRES_ENABLE_DRONE="$ENABLE_DRONE" \
  MULTIRES_UPLOAD_BUDGET_MB="$UPLOAD_BUDGET_MB" \
  MULTIRES_MSAA_SAMPLES="$MSAA_SAMPLES" \
  METAL_CAPTURE_ENABLED="$METAL_CAPTURE_ENABLED" \
  MULTIRES_METAL_CAPTURE_FRAME="$METAL_CAPTURE_FRAME" \
  MULTIRES_METAL_CAPTURE_DIR="$METAL_CAPTURE_DIR" \
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

  [[ -f "${base}.json" && -f "${base}.csv" ]] || {
    echo "Trial $trial did not produce expected benchmark outputs." >&2
    exit 1
  }

  if [[ -n "$capture_dir_trial" ]]; then
    IFS=',' read -r -a capture_frames_expected <<< "$CAPTURE_FRAMES"
    expected_count=0
    for frame in "${capture_frames_expected[@]}"; do
      frame_trimmed="$(echo "$frame" | xargs)"
      [[ -n "$frame_trimmed" ]] || continue
      expected_count=$((expected_count + 1))
      [[ -f "$capture_dir_trial/frame_$(printf '%06d' "$frame_trimmed").png" ]] || {
        echo "Missing expected capture for frame $frame_trimmed in $capture_dir_trial" >&2
        exit 1
      }
    done
    actual_count="$(find "$capture_dir_trial" -maxdepth 1 -name 'frame_*.png' | wc -l | tr -d ' ')"
    [[ "$actual_count" -ge "$expected_count" ]] || {
      echo "Capture count mismatch in $capture_dir_trial: expected at least $expected_count, found $actual_count" >&2
      exit 1
    }
  fi
done

summary_csv="$OUT_DIR/summary.csv"
summary_txt="$OUT_DIR/summary.txt"

echo "== Aggregating results =="
{
  echo "trial,scenario_name,overlays,present_mode,drone_enabled,debug_tools_enabled,perf_title_enabled,ready_wait_s,ready_atlas_count,ready_loaded_atlas_count,ready_loaded_tile_total,fps_mean,frame_ms_mean,frame_ms_p95,frame_ms_p99,frame_ms_max,frame_over_25ms_count,frame_over_33ms_count,frame_over_50ms_count,latency_estimate_ms,peak_rss_kib,msaa_samples,sample_count"
  for csv in "$OUT_DIR"/trial_*.csv; do
    trial_name="$(basename "$csv" .csv)"
    awk -F, -v trial="$trial_name" 'NR==2 {printf "%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n", trial, $1, $2, $3, $33, $8, $9, $13, $14, $15, $16, $17, $18, $22, $23, $24, $25, $26, $27, $28, $29, $30, $12}' "$csv"
  done
} > "$summary_csv"

awk -F, '
NR==1 {next}
{
  n++;
  ready_wait+=$8; fps+=$12; frame_mean+=$13; p95+=$14; p99+=$15; latency+=$20; rss+=$21;
  if ($14 > p95_max || n == 1) p95_max=$14;
  if ($15 > p99_max || n == 1) p99_max=$15;
  if ($13 < frame_best || n == 1) frame_best=$13;
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
  printf "frame_ms_p99_avg=%.6f\n", p99 / n;
  printf "latency_estimate_ms_avg=%.6f\n", latency / n;
  printf "peak_rss_kib_avg=%.0f\n", rss / n;
  printf "frame_ms_p95_worst=%.6f\n", p95_max;
  printf "frame_ms_p99_worst=%.6f\n", p99_max;
  printf "frame_ms_mean_best=%.6f\n", frame_best;
}' "$summary_csv" > "$summary_txt"

echo "Benchmark artifacts:"
echo "  $OUT_DIR"
echo "  $summary_csv"
echo "  $summary_txt"
