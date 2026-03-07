# Agent Orchestration Plan (2026-03-07)

## Mission
Bring `planetary_terrain_renderer` to a state where it is:
- build-stable across the workspace
- measured for physical correctness against `small_world`
- benchmarked with reproducible, renderer-focused evidence
- optimized for lower tail latency without sacrificing precision

## Shared Gates
- workspace build and test commands are green
- spatial correctness metrics are automated and rerunnable
- benchmark mode measures renderer work, not debug-only overhead
- heavy overlay scenario (`swiss`) improves on p95 and p99 without regressing correctness

## Agent Roster

| Task | Agent focus | Skill | Branch suggestion | Depends on |
|---|---|---|---|---|
| O1 | Benchmark contract and GPU attribution hardening | `terrain-benchmark-profiler` | `codex/benchmark-profiler` | none |
| O2 | Tile scheduling and backlog smoothing | `terrain-streaming-optimizer` | `codex/streaming-optimizer` | O1, O3 |
| O3 | Render and extract churn reduction | `terrain-render-path-optimizer` | `codex/render-path-optimizer` | O1 |
| O4 | Optimization safety verification | `terrain-release-verifier` | `codex/release-verifier` | O2, O3 |

## Execution Order

1. Execute O1 first. Do not start optimization until the benchmark contract, scenario labeling, and attribution path are stable.
2. O1 is now materially complete: scenario labeling, capture validation, measurement-window resets, and CPU-side phase attribution are live.
3. Launch O3 first on the accepted low-latency baseline.
4. Hold O2 behind O3 unless new low-latency measurements point back to streaming or upload pressure.
5. Require every optimization agent to preserve the current correctness and capture-validation gates.
6. Launch O4 only after O2-O3 have published before/after artifacts.

## Ownership Boundaries

- O1 owns benchmark/example/script/docs changes:
  - `examples/spherical_multires.rs`
  - `scripts/benchmark_spherical_multires.sh`
  - benchmark docs and findings
- O2 owns streaming and upload behavior:
  - `src/terrain_data/tile_loader.rs`
  - `src/terrain_data/tile_atlas.rs`
  - `src/terrain_data/gpu_tile_atlas.rs`
- O3 owns render and extraction churn:
  - `src/render/terrain_view_bind_group.rs`
  - `src/terrain_data/tile_tree.rs`
  - other render-path files only if needed

If a task needs to cross these boundaries, it must first update the plan and explain why the overlap is unavoidable.

## Task Packets

### O1: Benchmark Contract And GPU Attribution Hardening
Skill: `terrain-benchmark-profiler`
Goal:
- make optimization decisions reproducible and scenario-stable
Deliverables:
- canonical scenario naming and artifact schema
- canonical scenario matrix covering both performance and close-up visual validation
- capture validation in the benchmark wrapper
- pass-level or at least stronger phase-level attribution path
- refreshed baseline table with p95, p99, max, outlier counts, and memory watermark
Acceptance:
- optimization work can point to stable named scenarios and richer telemetry, not only frame-wide totals
- benchmark captures remain nonblank and required artifacts are enforced
- the canonical matrix includes at least:
  - a heavy-overlay moving sweep latency scenario
  - a heavy-overlay close-up visual smoke scenario
  - a base-only control scenario
- current status:
  - complete enough to gate O2 and O3
  - remaining gap is true GPU execution timing, not CPU-side phase attribution
Prompt:
- Extend the current benchmark and profiling workflow in `planetary_terrain_renderer` so the next optimization pass has a stable benchmark contract and richer attribution. Keep the benchmark renderer-focused and preserve capture validation.

### O2: Tile Scheduling And Upload Smoothing
Skill: `terrain-streaming-optimizer`
Goal:
- reduce tile churn and upload burstiness on the heavy overlay case
Deliverables:
- instrumentation for requests, releases, uploads, evictions, and backlog growth
- scheduling or residency changes
- before/after benchmark table for `swiss`
Acceptance:
- `swiss` moving-sweep p95 improves when the benchmark evidence points to streaming or upload pressure
- `swiss` moving-sweep p99 improves when the benchmark evidence points to streaming or upload pressure
- no correctness metric regression
Prompt:
- Optimize the terrain streaming path in `planetary_terrain_renderer`. Focus on prioritization, hysteresis, cancellation, upload budgeting, and atlas residency. Do not change geodesy or dataset semantics.
Current first target:
- no longer the active first target on the accepted low-latency baseline
- higher default upload budget (`24 MB/frame`) is still a validated tuning change
- upload-priority reordering was tested and rejected on the Swiss heavy-overlay baseline
- revisit this packet only if the `MSAA_SAMPLES=1` baseline or later GPU-attribution work points back to streaming or staging pressure

### O3: Render-Path Churn Reduction
Skill: `terrain-render-path-optimizer`
Goal:
- remove avoidable per-frame buffer writes, bind-group rebuilds, and resource churn
Deliverables:
- reduced hot-path churn
- updated benchmark and profile evidence
- memory impact summary
Acceptance:
- profile shows reduced `Queue::write_buffer*` or resource recreation on the hot path
- benchmark p95 or p99 improves without visual regressions
- render-side GPU-heavy work is isolated before deeper structural changes are made
Prompt:
- Optimize the render and extraction path in `planetary_terrain_renderer` using the existing CPU profile as a starting point. Focus on persistent bind groups, reduced full-buffer rewrites, and resource lifetime improvements. Keep benchmark captures validating rendered terrain.
Current first target:
- keep `MSAA_SAMPLES=1` as the low-latency benchmark baseline and `MSAA_SAMPLES=4` as the quality-control comparison
- the simplified terrain `relief_shading()` path in `src/shaders/attachments.wgsl` is now the accepted baseline because it moved the Swiss sweep from about `60.97 FPS / 16.40 ms mean / 25.21 ms p95` to about `99.55 FPS / 10.05 ms mean / 15.23 ms p95`
- reusing terrain depth textures across frames improved the default Swiss low-latency path further to about `103.76 FPS / 9.64 ms mean / 14.39 ms p95`
- terrain blending was measured as a modest but non-compelling win and is not the next accepted default change
- caching the depth-copy bind group and reusing the cached depth view were both tested and rejected as insufficiently robust follow-ups
- next work should isolate terrain depth copy and remaining main-opaque-pass cost rather than returning to upload heuristics prematurely

### O4: Integration And Merge Gate Verification
Skill: `terrain-release-verifier`
Goal:
- validate that the integrated result is ready for merge
Deliverables:
- gate table with pass/fail for build, correctness, benchmark, and captures
- concise go or no-go summary
Acceptance:
- all required gates pass, or exact blockers are named
Prompt:
- Validate the integrated `planetary_terrain_renderer` changes against build, correctness, benchmark, and visual gates. Produce a concise go or no-go summary with the exact commands and artifacts used.

## Current Baseline To Beat

- workspace build/test baseline is green
- direct renderer geodesy matches `small_world`
- current Earth `lod_count = 5` build is close to the dataset floor in flat/coastal regions
- rebuilt Swiss overlay is in the `~13-20 m` p95 class against its source raster in the local HGT-overlap strip
- Swiss renderer low-latency benchmark baseline from the capture-validated sweep:
  - artifacts: `/tmp/terrain_depth_cache_confirm/summary.txt`
  - FPS mean `103.76`
  - frame mean `9.64 ms`
  - p95 `14.39 ms`
  - p99 `17.31 ms`
- Swiss quality-control comparison:
  - artifacts: `/tmp/terrain_bench_msaa4_isolation/summary.txt`
  - FPS mean `36.86`
  - frame mean `27.17 ms`
  - p95 `43.91 ms`
  - p99 `51.27 ms`

## Still Relevant Risks

- missing true GPU pass attribution still means some optimization choices are guided by inference
- the heaviest overlay case still needs a clear quality policy between the low-latency `MSAA_SAMPLES=1` baseline and the `MSAA_SAMPLES=4` comparison
- local `small_world` coverage for mountainous truth work is still geographically narrow
