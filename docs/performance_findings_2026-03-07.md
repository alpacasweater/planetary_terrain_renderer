# Spherical Multires Performance Findings (2026-03-07)

## Scope
Rigorous benchmark pass for `examples/spherical_multires.rs` with:
- moving benchmark camera sweep (crosses overlay boundary to include high-res and low-res terrain)
- automatic PNG capture during each trial
- 5 scenarios x 3 trials each
- warmup 6s, measure 15s

Run root:
- `/tmp/terrain_bench_rigorous_sweep_capture_20260307_010416`

## Scenarios
- `base_only_fast`: `OVERLAYS=none`, `PRESENT_MODE=auto_novsync`
- `swiss_fast`: `OVERLAYS=swiss`, `PRESENT_MODE=auto_novsync`
- `saxony_fast`: `OVERLAYS=saxony`, `PRESENT_MODE=auto_novsync`
- `los_fast`: `OVERLAYS=los`, `PRESENT_MODE=auto_novsync`
- `swiss_vsync`: `OVERLAYS=swiss`, `PRESENT_MODE=auto_vsync`

## Aggregate Results (per-scenario averages over 3 trials)

| Scenario | FPS mean | FPS stdev | frame ms mean | p95 mean (ms) | worst p95 (ms) | ready wait mean (s) |
|---|---:|---:|---:|---:|---:|---:|
| base_only_fast | 49.38 | 0.45 | 20.253 | 32.128 | 32.416 | 0.157 |
| los_fast | 57.66 | 0.04 | 17.342 | 20.061 | 20.204 | 0.024 |
| saxony_fast | 57.31 | 0.22 | 17.449 | 20.864 | 21.531 | 0.065 |
| swiss_fast | 40.72 | 1.39 | 24.586 | 39.970 | 40.506 | 0.097 |
| swiss_vsync | 49.48 | 4.24 | 20.350 | 25.493 | 28.072 | 0.025 |

## Visual Validation (PNG captures)
All trials produced captures with non-trivial image variance (not blank frames):
- minimum grayscale stddev by scenario/trial was >10
- unique grayscale levels per image ranged ~160-233
- no capture was flagged blank-like

## Deficiencies vs “Snappy / Low-Latency” Goals
1. Tail latency is the main problem, not average FPS.
- In heavy Swiss overlay, p95 ~40ms (25 FPS-class tail), with visible jitter risk.
- Even base-only p95 ~32ms indicates frame pacing instability under movement/LOD change.

2. Overlay-dependent performance spread is large.
- `swiss_fast` is significantly worse than `saxony_fast`/`los_fast` on both mean and p95.
- This points to workload sensitivity to tile density/complexity and/or churn policy.

3. Warmup/startup spikes are still high.
- Early-frame spikes >100ms appeared repeatedly before settling.
- This degrades perceived responsiveness when launching or changing view.

4. Frame pacing remains uneven under camera sweep.
- Even with stable ready-state, periodic spikes persist.
- “Snappy feel” suffers despite moderate average FPS.

5. Benchmark reproducibility was previously fragile.
- Fixed now by asset-root and automated PNG capture, but benchmark mode still shares runtime paths with interactive debug toggles.

## Precision-Safe Optimization Plan
Goal: reduce latency variance and tail (p95/p99), preserve geodesy/math correctness.

### Phase 1: Deterministic benchmarking and observability (low risk)
- Add dedicated benchmark mode gate that disables runtime debug hotkeys/toggles only (not geodesy/math).
- Record additional metrics per frame:
  - raw `dt` p99/max (not only smoothed diagnostics)
  - tile request/release counts per second
  - GPU frame time if available from backend counters
- Extend benchmark output with:
  - p99/max frame time
  - outlier count above thresholds (e.g. >25ms, >33ms, >50ms)

Expected impact: no precision change; better diagnosis and stable perf testing.

### Phase 2: Reduce tile churn and burstiness (precision-neutral)
- Add/request hysteresis around LOD transition distances to reduce request/release thrash during motion.
- Rate-limit tile request bursts per frame (token bucket), smoothing CPU/GPU upload workload.
- Pre-warm likely-visible neighbor tiles in background at lower priority.

Expected impact: lower p95/p99 and fewer stutters; same coordinate precision and terrain accuracy.

### Phase 3: Upload/render pipeline smoothing (precision-neutral)
- Move expensive upload/staging work off critical frame path where possible.
- Cap per-frame GPU upload bytes to avoid occasional long frames.
- Evaluate atlas residency policy to retain recently-used tiles longer in heavy overlays.

Expected impact: improved frame pacing with unchanged geospatial correctness.

### Phase 4: Camera-motion-aware scheduling (precision-neutral)
- Predict short-horizon camera trajectory and prioritize tiles in motion direction.
- Downgrade eviction priority for tiles likely to re-enter view soon.

Expected impact: smoother motion in sweep/drone-like paths; no geodesy precision tradeoff.

### Phase 5: Optional quality/perf knobs (explicitly non-default)
- Add optional benchmark/runtime tuning profile for reduced detail in non-critical far-field only.
- Keep physically-correct mapping and high-precision transforms untouched as default.

Expected impact: configurable speedups without reducing physical correctness unless user opts in.

## Recommendation
Prioritize Phase 1 + 2 immediately. They target the observed bottleneck (tail latency and jitter) and do not alter geodetic math, coordinate transforms, or terrain truth-model precision.
