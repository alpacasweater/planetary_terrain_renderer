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
- `fps_mean`
- `frame_ms_mean`
- `frame_ms_p50/p90/p95/p99`
- `latency_estimate_ms` (currently `p95`)
- `ready_wait_s` (time spent waiting for terrain tiles to be loaded before warmup starts)
- `sample_count`

After all trials, the script writes:
- `summary.csv`: one row per trial
- `summary.txt`: aggregate statistics (mean and worst/best)

## Default Benchmark Settings

- `TRIALS=3`
- `WARMUP_SECONDS=8`
- `DURATION_SECONDS=20`
- `READY_TIMEOUT_SECONDS=120` (script-level env passed to example as `MULTIRES_BENCHMARK_READY_TIMEOUT_SECONDS`)
- `PRESENT_MODE=auto_novsync`
- `OVERLAYS=swiss`
- `CAMERA_ALT_M=90000`
- `CAMERA_BACKOFF_M=150000`
- `CAPTURE_DIR` unset by default (set to enable PNG captures)
- `CAPTURE_FRAMES=120,360,720` (frame numbers to capture when `CAPTURE_DIR` is set)

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

## Direct Example Benchmark Mode (without script)

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
- Benchmarking now waits until terrain atlases report loaded tiles before warmup/measurement.
- The script sets `BEVY_ASSET_ROOT` so release-binary launches resolve project `assets/` reliably.
- The example supports PNG capture envs directly:
  `MULTIRES_CAPTURE_DIR` and `MULTIRES_CAPTURE_FRAMES`.
- Use fixed overlay/camera settings when comparing runs.
- Keep machine load stable; close heavy background tasks to reduce jitter.
