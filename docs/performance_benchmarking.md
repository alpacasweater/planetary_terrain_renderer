# Spherical Multires Performance Benchmarking

This project includes an automated benchmark mode in `examples/spherical_multires.rs` and a runner script.

The benchmark contract is:
- benchmark scenarios must be named and recorded in artifacts
- drone rendering is off by default in benchmark mode
- capture-enabled runs must emit the expected PNG files or fail
- phase timing telemetry and perf counters are reset at measurement start, so benchmark artifacts describe the measurement window rather than startup or warmup

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
- `scenario_name`
- `overlays`
- `present_mode`
- `benchmark_mode`
- `debug_tools_enabled`
- `perf_title_enabled`
- `fps_mean`
- `frame_ms_mean`
- `frame_ms_p50/p90/p95/p99`
- `frame_over_25ms_count`
- `frame_over_33ms_count`
- `frame_over_50ms_count`
- `latency_estimate_ms` (currently `p95`)
- `peak_rss_kib`
- `ready_wait_s`
- `ready_atlas_count`
- `ready_loaded_atlas_count`
- `ready_loaded_tile_total`
- `hottest_phase_name`
- `hottest_phase_mean_ms`
- `hottest_phase_p95_ms`
- `hottest_phase_max_ms`
- `phase_timings` (JSON only; per-phase mean, p95, p99, max, sample_count)
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
- `msaa_samples`
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
- `ENABLE_DRONE=0`
- `UPLOAD_BUDGET_MB=24`
- `MSAA_SAMPLES=4`
- `EXAMPLE_FEATURES` unset by default
- `METAL_CAPTURE_ENABLED=0`
- `METAL_CAPTURE_FRAME` unset by default
- `METAL_CAPTURE_DIR` unset by default
- `BENCHMARK_SWEEP_DEG=8`
- `BENCHMARK_SWEEP_PERIOD_SECONDS=40`

## Useful Overrides

```bash
TRIALS=5 OVERLAYS=saxony WARMUP_SECONDS=10 DURATION_SECONDS=30 ./scripts/benchmark_spherical_multires.sh
```

```bash
OUT_DIR=/tmp/terrain_bench PRESENT_MODE=auto_vsync ./scripts/benchmark_spherical_multires.sh
```

```bash
OUT_DIR=/tmp/terrain_bench_msaa1 \
MSAA_SAMPLES=1 \
./scripts/benchmark_spherical_multires.sh
```

```bash
OUT_DIR=/tmp/terrain_bench \
CAPTURE_DIR=/tmp/terrain_bench_captures \
CAPTURE_FRAMES=240,480 \
./scripts/benchmark_spherical_multires.sh
```

Named scenario:

```bash
SCENARIO_NAME=swiss_fast_sweep \
OVERLAYS=swiss \
PRESENT_MODE=auto_novsync \
./scripts/benchmark_spherical_multires.sh
```

## Recommended Scenario Matrix

Use at least these three scenarios when evaluating optimization changes:
- `swiss_fast_sweep`: moving heavy-overlay latency scenario
- `swiss_close_smoke`: close-up heavy-overlay visual-validation scenario
- `base_only_fast`: base-earth control scenario

Reason:
- the moving sweep is the main latency benchmark
- the close-up smoke run is better for validating that the overlay and any demo entities are actually visible
- the base-only control helps separate overlay costs from general renderer costs

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

Metal GPU trace capture:

```bash
OUT_DIR=/tmp/terrain_bench_gpu_trace \
TRIALS=1 \
WARMUP_SECONDS=2 \
DURATION_SECONDS=5 \
EXAMPLE_FEATURES=metal_capture \
METAL_CAPTURE_ENABLED=1 \
METAL_CAPTURE_FRAME=120 \
METAL_CAPTURE_DIR=/tmp/terrain_bench_gpu_trace_gputrace \
CAPTURE_DIR=/tmp/terrain_bench_gpu_trace_captures \
CAPTURE_FRAMES=90 \
./scripts/benchmark_spherical_multires.sh
```

This writes:
- benchmark JSON/CSV under `OUT_DIR`
- a `.gputrace` directory under `METAL_CAPTURE_DIR`
- optional PNG captures under `CAPTURE_DIR`

Important:
- `METAL_CAPTURE_ENABLED=1` is required for document capture outside Xcode
- do not schedule `CAPTURE_FRAMES` on the same frame as `METAL_CAPTURE_FRAME`; the runner will reject that configuration

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
- Phase timing telemetry and tile request or upload counters are reset when measurement begins.
- The script sets `BEVY_ASSET_ROOT` so release-binary launches resolve project `assets/` reliably.
- The runner disables debug/picking and title-update overhead unless re-enabled with `ENABLE_DEBUG_TOOLS=1` or `ENABLE_PERF_TITLE=1`.
- Upload pressure is limited by `UPLOAD_BUDGET_MB` in the runner and `MULTIRES_UPLOAD_BUDGET_MB` in the example. Set it to `0` to benchmark without throttling.
- MSAA is controlled by `MSAA_SAMPLES` in the runner and `MULTIRES_MSAA_SAMPLES` in the example.
- Current CPU-side phase attribution shows the Swiss sweep is dominated by `render.prepare.gpu_tile_atlas`, and within that phase by texture uploads rather than mip bind-group setup.
- Short Swiss isolation runs on this machine showed `MSAA_SAMPLES=1` materially outperforms `MSAA_SAMPLES=4`, so MSAA should be treated as a first-class benchmark dimension rather than an implicit default.
- The example supports PNG capture envs directly: `MULTIRES_CAPTURE_DIR` and `MULTIRES_CAPTURE_FRAMES`.
- The example also supports one-shot Metal capture envs when built with `--features metal_capture`: `MULTIRES_METAL_CAPTURE_FRAME` and `MULTIRES_METAL_CAPTURE_DIR`.
- Use fixed overlay and camera settings when comparing runs.
- GPU-backed runs are required for meaningful captures and timings; sandboxed runs on this machine do not expose a GPU.
