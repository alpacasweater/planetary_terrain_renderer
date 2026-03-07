# Spherical Multires Performance Findings (2026-03-07)

## Scope
Rigorous benchmark pass for `examples/spherical_multires.rs` with:
- moving benchmark camera sweep
- automatic PNG capture during each trial
- 5 scenarios x 3 trials each
- warmup 6s, measure 15s

Run root:
- `/tmp/terrain_bench_rigorous_sweep_capture_20260307_010416`

## Aggregate Results

| Scenario | FPS mean | frame ms mean | p95 mean (ms) | worst p95 (ms) | ready wait mean (s) |
|---|---:|---:|---:|---:|---:|
| base_only_fast | 49.38 | 20.253 | 32.128 | 32.416 | 0.157 |
| los_fast | 57.66 | 17.342 | 20.061 | 20.204 | 0.024 |
| saxony_fast | 57.31 | 17.449 | 20.864 | 21.531 | 0.065 |
| swiss_fast | 40.72 | 24.586 | 39.970 | 40.506 | 0.097 |
| swiss_vsync | 49.48 | 20.350 | 25.493 | 28.072 | 0.025 |

## Visual Validation

All trials produced nonblank captures.

## Snappy-Execution Deficiencies

1. Tail latency is the main problem, not average FPS.
- Heavy Swiss overlay stays around `~40 ms` p95 in the capture-validated sweep.

2. Overlay-dependent spread is still large.
- `swiss_fast` remains substantially worse than `saxony_fast` and `los_fast`.

3. Warmup and startup spikes are still high.
- Early-frame spikes above `100 ms` remain visible.

4. Frame pacing is still uneven under motion.
- The heavy overlay case still has visible jitter headroom even when terrain is loaded.

## Follow-up Experiment: Tile-Tree Upload Gating

Change:
- `TileTree::update_terrain_view_buffer` now skips the large `tile_tree_buffer` upload when the best-tile entries are unchanged.

Measurement:
- baseline run: `/tmp/terrain_bench_budget16/trial_1.json`
- follow-up run: `/tmp/terrain_bench_tiletree_counters_fixed/trial_1.json`

Observed metrics:
- baseline `fps_mean`: `49.42`
- follow-up `fps_mean`: `48.89`
- baseline `frame_ms_p95`: `26.196`
- follow-up `frame_ms_p95`: `29.704`
- follow-up `tile_tree_buffer_updates_total`: `29`
- follow-up `tile_tree_buffer_skipped_total`: `665`

Conclusion:
- the optimization was active
- it did not improve frame-time tails materially
- tile-tree storage uploads are not the primary latency lever

## Current Optimization Status

Completed:
- workspace build/test baseline is green
- benchmark mode is stable and capture-validated
- correctness tooling is in place, so performance work can be checked against physical-truth gates
- tile-tree upload gating was tested and shown not to be the primary latency lever
- phase attribution now identifies the current Swiss heavy-overlay hotspot instead of only whole-frame tails

Open bottlenecks:
1. tile streaming churn in heavy overlays
2. upload backlog spikes and staging pressure
3. render-path buffer and bind-group churn
4. true GPU execution timing is still missing; current attribution is CPU-side only

## Current Optimization Priority

1. reduce CPU-side texture upload cost in `render.prepare.gpu_tile_atlas`
2. add stronger tile request prioritization, hysteresis, and stale-load cancellation
3. eliminate avoidable render and extract buffer rewrites and bind-group recreation
4. add GPU timing or capture-backed attribution so future tuning targets measured passes, not guesses

## O1 Attribution Update

Benchmark artifact:
- `/tmp/terrain_bench_phase_smoke2/trial_1.json`

Key Swiss sweep findings from the measurement-window-scoped phase telemetry:
- whole-frame:
  - FPS mean `34.52`
  - frame p95 `34.202 ms`
  - frame p99 `155.290 ms`
- hottest CPU phase:
  - `render.prepare.gpu_tile_atlas`
  - mean `2.983 ms`
  - p95 `10.111 ms`
  - p99 `18.328 ms`
- split inside that phase:
  - `render.prepare.gpu_tile_atlas.uploads`
    - mean `2.512 ms`
    - p95 `8.645 ms`
    - p99 `15.785 ms`
  - `render.prepare.gpu_tile_atlas.mip_bind_groups`
    - mean `0.471 ms`
    - p95 `1.875 ms`
    - p99 `2.746 ms`

Interpretation:
- the dominant measured CPU bottleneck is texture upload work, not mip bind-group preparation
- render graph node encoding and tile-tree buffer updates are secondary in this scenario

## O2 First Tuning: Higher Default Upload Budget

Short multi-trial comparison:
- `16 MB/frame`: `/tmp/terrain_bench_budget_compare_16/summary.csv`
- `24 MB/frame`: `/tmp/terrain_bench_budget_compare_24/summary.csv`

Observed averages across 3 trials, `warmup=2s`, `measure=6s`:
- `16 MB/frame`
  - FPS `38.60`
  - p95 `47.255 ms`
  - p99 `61.462 ms`
  - worst p95 `50.410 ms`
- `24 MB/frame`
  - FPS `42.55`
  - p95 `37.012 ms`
  - p99 `49.761 ms`
  - worst p95 `39.502 ms`

Action taken:
- default `TerrainSettings::upload_budget_bytes_per_frame` raised from `16 MB` to `24 MB`

Assessment:
- this is a safe first tuning change
- it improves the measured Swiss heavy-overlay sweep without touching correctness semantics
- it does not remove the upload hotspot; it only reduces how badly the current path stalls

## O2 Experiment Rejected: Priority-Aware Upload Queue

Change tested:
- reorder pending GPU upload tiles by tile request priority and attachment usefulness before applying the frame upload budget

Comparison artifacts:
- baseline `24 MB/frame`: `/tmp/terrain_bench_budget_compare_24/summary.txt`
- priority-aware queue: `/tmp/terrain_bench_priority_compare_24/summary.txt`

Observed averages across 3 trials:
- baseline `24 MB/frame`
  - FPS `42.55`
  - frame mean `23.511 ms`
  - p95 `37.012 ms`
  - p99 `49.761 ms`
- priority-aware queue
  - FPS `42.32`
  - frame mean `23.645 ms`
  - p95 `37.898 ms`
  - p99 `47.927 ms`

Assessment:
- the change slightly improved p99 but made average p95 worse
- it also increased average peak upload backlog (`2.0 -> 4.33`)
- the added complexity is not justified by the measured result

Action taken:
- the priority-aware upload queue was removed
- the validated `24 MB/frame` default remains

## O3 Preparation: Automated Metal GPU Trace Capture

What was added:
- feature-gated Metal capture automation via:
  - `MULTIRES_METAL_CAPTURE_FRAME`
  - `MULTIRES_METAL_CAPTURE_DIR`
- benchmark runner support via:
  - `EXAMPLE_FEATURES`
  - `METAL_CAPTURE_ENABLED`
  - `METAL_CAPTURE_FRAME`
  - `METAL_CAPTURE_DIR`

Validation artifacts:
- benchmark + trace run:
  - benchmark JSON: `/tmp/terrain_metal_capture_smoke2/trial.json`
  - trace: `/tmp/terrain_metal_capture_smoke2_gputrace/swiss_metal_capture_smoke2_1772919561.gputrace`

Important runtime constraints:
- `METAL_CAPTURE_ENABLED=1` is required for document capture outside Xcode
- PNG screenshot capture should not target the same frame as Metal capture

Implication:
- the next optimization pass should use GPU trace evidence instead of continuing to optimize CPU-side upload ordering heuristically

## O3 Isolation: MSAA As A First-Order Latency Lever

The Metal capture exposed a small pass inventory for the benchmarked frame:
- repeated `(wgpu internal) Pre Pass`
- `terrain_pass`
- `main_opaque_pass_3d`
- `early_mesh_preprocessing`
- `late_mesh_preprocessing`
- `upscaling`

That made MSAA and the terrain depth path worth isolating directly.

Short 2-trial Swiss moving-sweep comparison (`warmup=6s`, `measure=15s`, `UPLOAD_BUDGET_MB=24`):
- `MSAA_SAMPLES=4`
  - artifacts: `/tmp/terrain_bench_msaa4_isolation/summary.txt`
  - FPS mean `36.86`
  - frame mean `27.173 ms`
  - p95 `43.907 ms`
  - p99 `51.266 ms`
- `MSAA_SAMPLES=1`
  - artifacts: `/tmp/terrain_bench_msaa1_isolation/summary.txt`
  - FPS mean `58.04`
  - frame mean `17.230 ms`
  - p95 `26.377 ms`
  - p99 `35.934 ms`

Interpretation:
- on this machine, MSAA is not a marginal quality toggle; it is a major frame-time cost in the heavy Swiss overlay scene
- `MSAA_SAMPLES=1` improved the benchmark by roughly:
  - `+57%` FPS
  - `-10 ms` mean frame time
  - `-17.5 ms` p95
  - `-15.3 ms` p99

Additional finding:
- the terrain depth-copy path was incorrectly hard-coded for multisampled depth textures and a 4x copy pipeline
- this made `MSAA_SAMPLES=1` invalid before the fix
- the renderer now has both single-sample and multisampled depth-copy variants, selected from the view's actual `Msaa` component

Action:
- keep MSAA configurable in the benchmark harness
- treat `MSAA_SAMPLES=1` as the low-latency benchmark configuration for future optimization work unless image-quality validation says otherwise
- keep `MSAA_SAMPLES=4` as the quality-control comparison
