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
1. true GPU execution timing is still missing; remaining work is increasingly GPU-bound
2. terrain depth and main-pass overhead still need controlled isolation on the low-latency baseline
3. asset noise from missing Earth albedo tiles still pollutes profiling output and visual runs
4. CPU upload and staging work should be revisited only if later low-latency baselines point back there

## Current Optimization Priority

1. keep `MSAA_SAMPLES=1` as the canonical low-latency benchmark configuration and `MSAA_SAMPLES=4` as the quality-control comparison
2. reduce terrain fragment-shading cost and other GPU-heavy render work before returning to streaming heuristics
3. isolate terrain depth, main opaque pass, and remaining render-path overhead on the new low-latency baseline
4. add stronger GPU timing or capture-backed attribution so future tuning targets measured passes, not guesses

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

## O4 Isolation: Terrain Relief Shading Was The Dominant GPU-Side Lever

The accepted `MSAA_SAMPLES=1` baseline changed the optimization picture enough that the old CPU upload hotspot stopped being the main latency lever.

Rebaselined Swiss moving sweep (`warmup=3s`, `measure=6s`, `UPLOAD_BUDGET_MB=24`, `MSAA_SAMPLES=1`):
- artifacts: `/tmp/terrain_bench_swiss_msaa1_rebaseline/summary.txt`
- aggregate across 3 trials:
  - FPS `61.19`
  - frame mean `16.360 ms`
  - p95 `26.650 ms`
  - p99 `32.670 ms`

Key per-trial observation:
- the hottest measured CPU phase was still reported as `render.prepare.gpu_tile_atlas`
- but its p95 was only about `0.52 ms`

Interpretation:
- on the accepted low-latency baseline, CPU upload work was no longer large enough to explain frame-wide latency
- the next useful isolation had to move back to render-side GPU-sensitive work

Controlled shader isolation on that same baseline:
- default terrain lighting enabled:
  - artifact: `/tmp/swiss_msaa1_default_iso.json`
  - FPS `60.97`
  - frame mean `16.402 ms`
  - p95 `25.214 ms`
- terrain lighting disabled:
  - artifact: `/tmp/swiss_msaa1_unlit_iso.json`
  - FPS `114.07`
  - frame mean `8.766 ms`
  - p95 `13.426 ms`

Interpretation:
- terrain fragment shading, not streaming, was the dominant GPU-side lever in the Swiss demo path
- the example shader was using an expensive stochastic multi-light `relief_shading()` path

Implemented fix:
- simplified `relief_shading()` in `src/shaders/attachments.wgsl`
- replaced the 4-sample pseudo-random lighting loop with a cheap single-direction direct term plus hemisphere fill
- preserved lighting-on rendering rather than relying on an unlit fallback

Post-fix Swiss moving sweep (`MSAA_SAMPLES=1`, lighting enabled):
- artifact: `/tmp/swiss_msaa1_relief_fast.json`
- capture: `/tmp/swiss_msaa1_relief_fast_captures/frame_000120.png`
- capture validation:
  - grayscale mean `55.07`
  - grayscale stddev `17.24`
- benchmark result:
  - FPS `99.55`
  - frame mean `10.045 ms`
  - p95 `15.235 ms`
  - p99 `19.088 ms`
  - max `24.930 ms`

Compared to the pre-fix lighting-on baseline:
- FPS: `60.97 -> 99.55`
- frame mean: `16.402 ms -> 10.045 ms`
- p95: `25.214 ms -> 15.235 ms`
- p99: `30.362 ms -> 19.088 ms`

Current implication:
- the main accepted low-latency path is now much closer to a genuinely snappy baseline
- future optimization should start from this lighter terrain-shading path
- the next likely targets are terrain depth work, remaining main-pass overhead, and better GPU attribution, not upload reordering

## O5 Isolation: Terrain Blending Was Not Strong Enough To Promote

Short feature-toggle isolation on the accepted `MSAA_SAMPLES=1` baseline:
- default:
  - FPS `101.11`
  - frame mean `9.890 ms`
  - p95 `15.271 ms`
- `TERRAIN_BLEND=0`
  - FPS `105.62`
  - frame mean `9.468 ms`
  - p95 `13.016 ms`
- `TERRAIN_MORPH=0`
  - FPS `91.47`
  - frame mean `10.932 ms`
  - p95 `18.318 ms`
- `TERRAIN_SAMPLE_GRAD=0`
  - FPS `99.99`
  - frame mean `10.001 ms`
  - p95 `15.661 ms`
- `TERRAIN_HIGH_PRECISION=0`
  - FPS `78.92`
  - frame mean `12.672 ms`
  - p95 `19.371 ms`

Interpretation:
- `morph_off`, `sample_grad_off`, and `high_precision_off` were clearly not the next optimization path
- `blend_off` looked promising in one moving-sweep run, but that was not enough to justify a default-quality change

Close-up visual A/B (`CAMERA_ALT_M=15000`, `CAMERA_BACKOFF_M=25000`, sweep disabled):
- default:
  - FPS `69.42`
  - p95 `20.446 ms`
  - capture: `/tmp/terrain_blend_visual_captures/default/trial_1/frame_000060.png`
- `blend_off`:
  - FPS `70.30`
  - p95 `21.356 ms`
  - capture: `/tmp/terrain_blend_visual_captures/blend_off/trial_1/frame_000060.png`

Follow-up 3-trial confirmation:
- default:
  - artifact: `/tmp/terrain_blend_confirm/default/summary.txt`
  - FPS `99.31`
  - frame mean `10.076 ms`
  - p95 `15.266 ms`
  - p99 `18.746 ms`
- `blend_off`:
  - artifact: `/tmp/terrain_blend_confirm/blend_off/summary.txt`
  - FPS `102.65`
  - frame mean `9.746 ms`
  - p95 `14.730 ms`
  - p99 `17.237 ms`

Decision:
- keep blending enabled by default
- the gain is real but modest, and the close-up view did not make the trade compelling enough to change the renderer contract
- blending remains a diagnostic isolation toggle, not the next accepted optimization

## O6 Optimization: Cache Terrain Depth Textures Across Frames

The CPU-side hottest phase after the shading fix was often `render.prepare_resources.terrain_depth_textures`, but at only about `0.18 ms p95`.

Even so, it was still rebuilding and reinserting `TerrainViewDepthTexture` components every frame, including fresh depth/stencil views for unchanged targets. That is unnecessary render-path churn.

Implemented fix:
- `TerrainViewDepthTexture` now stores the view size and sample count
- `prepare_terrain_depth_textures()` reuses the existing component when the physical target size and MSAA sample count are unchanged

Before/after 3-trial Swiss default comparison:
- pre-fix:
  - artifact: `/tmp/terrain_blend_confirm/default/summary.txt`
  - FPS `99.31`
  - frame mean `10.076 ms`
  - p95 `15.266 ms`
  - p99 `18.746 ms`
- post-fix:
  - artifact: `/tmp/terrain_depth_cache_confirm/summary.txt`
  - FPS `103.76`
  - frame mean `9.645 ms`
  - p95 `14.387 ms`
  - p99 `17.314 ms`

Effect:
- FPS: `99.31 -> 103.76`
- frame mean: `10.076 ms -> 9.645 ms`
- p95: `15.266 ms -> 14.387 ms`
- p99: `18.746 ms -> 17.314 ms`

New CPU-side hottest phase after this fix:
- `render.prepare.gpu_terrain`
- p95 about `0.08-0.10 ms`

Interpretation:
- the depth-texture churn fix is worth keeping
- CPU-side prep phases on the accepted low-latency baseline are now deep into the sub-millisecond range
- the next meaningful work is even more clearly on GPU-pass attribution and render-path cost, not streaming heuristics
