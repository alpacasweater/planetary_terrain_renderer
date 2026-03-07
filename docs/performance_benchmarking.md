# Spherical Multires Performance Benchmarking

This project includes an automated benchmark mode in `examples/spherical_multires.rs` and a runner script.

## Quick Run

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
./scripts/benchmark_spherical_multires.sh
```

## What It Measures

Per trial, the example writes:
- `trial_N.json`: machine-readable benchmark summary
- `trial_N.csv`: one-row CSV summary

Metrics include:
- `benchmark_mode`
- `debug_tools_enabled`
- `perf_title_enabled`
- `fps_mean`
- `frame_ms_mean`
- `frame_ms_p50/p90/p95/p99`
- `latency_estimate_ms` (currently `p95`)
- `ready_wait_s`
- `ready_atlas_count`
- `ready_loaded_atlas_count`
- `ready_loaded_tile_total`
- `upload_budget_bytes_per_frame`
- `terrain_view_buffer_updates_total`
- `tile_tree_buffer_updates_total`
- `tile_tree_buffer_skipped_total`
- `tile_requests_total`
- `tile_releases_total`
- `canceled_pending_attachment_loads_total`
- `canceled_inflight_attachment_loads_total`
- `finished_attachment_loads_total`
- `upload_enqueued_attachment_tiles_total`
- `upload_enqueued_bytes_total`
- `upload_deferred_attachment_tiles_total`
- `peak_pending_attachment_queue`
- `peak_inflight_attachment_loads`
- `peak_upload_backlog_attachment_tiles`
- `sample_count`

After all trials, the script writes:
- `summary.csv`: one row per trial
- `summary.txt`: aggregate statistics
- optional PNG captures under `CAPTURE_DIR`

## Default Benchmark Settings

- `TRIALS=3`
- `WARMUP_SECONDS=8`
- `DURATION_SECONDS=20`
- `READY_TIMEOUT_SECONDS=120`
- `PRESENT_MODE=auto_novsync`
- `OVERLAYS=swiss`
- `CAMERA_ALT_M=90000`
- `CAMERA_BACKOFF_M=150000`
- `CAPTURE_DIR` unset by default
- `CAPTURE_FRAMES=120,360,720`
- `ENABLE_DEBUG_TOOLS=0`
- `ENABLE_PERF_TITLE=0`
- `UPLOAD_BUDGET_MB=16`

## Useful Overrides

```bash
TRIALS=5 OVERLAYS=saxony WARMUP_SECONDS=10 DURATION_SECONDS=30 ./scripts/benchmark_spherical_multires.sh
```

```bash
OUT_DIR=/tmp/terrain_bench PRESENT_MODE=auto_vsync ./scripts/benchmark_spherical_multires.sh
```

```bash
OUT_DIR=/tmp/terrain_bench \
CAPTURE_DIR=/tmp/terrain_bench_captures \
CAPTURE_FRAMES=240,480 \
./scripts/benchmark_spherical_multires.sh
```

Single-run capture for visual validation:

```bash
OUT_DIR=/tmp/terrain_bench_smoke \
TRIALS=1 \
WARMUP_SECONDS=2 \
DURATION_SECONDS=8 \
CAPTURE_DIR=/tmp/terrain_bench_smoke_captures \
CAPTURE_FRAMES=60 \
./scripts/benchmark_spherical_multires.sh
```

## Direct Example Benchmark Mode

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
RUST_LOG=perf=info \
MULTIRES_OVERLAYS=swiss \
MULTIRES_PRESENT_MODE=auto_novsync \
MULTIRES_BENCHMARK_READY_TIMEOUT_SECONDS=120 \
MULTIRES_BENCHMARK_WARMUP_SECONDS=8 \
MULTIRES_BENCHMARK_DURATION_SECONDS=20 \
MULTIRES_BENCHMARK_OUTPUT=/tmp/multires_trial \
cargo run --release --example spherical_multires
```

This writes:
- `/tmp/multires_trial.json`
- `/tmp/multires_trial.csv`

## Notes

- `auto_novsync` is preferred for throughput benchmarking.
- Benchmarking waits until terrain atlases report loaded tiles before warmup and measurement.
- The script sets `BEVY_ASSET_ROOT` so release-binary launches resolve project `assets/` reliably.
- The runner disables debug/picking and title-update overhead unless re-enabled with `ENABLE_DEBUG_TOOLS=1` or `ENABLE_PERF_TITLE=1`.
- Upload pressure is limited by `UPLOAD_BUDGET_MB` in the runner and `MULTIRES_UPLOAD_BUDGET_MB` in the example. Set it to `0` to benchmark without throttling.
- The example supports PNG capture envs directly: `MULTIRES_CAPTURE_DIR` and `MULTIRES_CAPTURE_FRAMES`.
- Use fixed overlay and camera settings when comparing runs.
- GPU-backed runs are required for meaningful captures and timings; sandboxed runs on this machine do not expose a GPU.
