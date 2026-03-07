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

Open bottlenecks:
1. tile streaming churn in heavy overlays
2. upload backlog spikes and staging pressure
3. render-path buffer and bind-group churn
4. missing GPU pass attribution for the worst frame-time tails

## Current Optimization Priority

1. add stronger tile request prioritization, hysteresis, and stale-load cancellation
2. reduce upload burstiness and backlog spikes
3. eliminate avoidable render and extract buffer rewrites and bind-group recreation
4. add GPU timing or capture-backed attribution so future tuning targets measured passes, not guesses
