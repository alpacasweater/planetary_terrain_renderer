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
| O1 | Tile scheduling and backlog smoothing | `terrain-streaming-optimizer` | `codex/streaming-optimizer` | none |
| O2 | Render and extract churn reduction | `terrain-render-path-optimizer` | `codex/render-path-optimizer` | none |
| O3 | GPU and benchmark attribution hardening | `terrain-benchmark-profiler` | `codex/benchmark-profiler` | none |
| O4 | Optimization safety verification | `terrain-release-verifier` | `codex/release-verifier` | O1, O2, O3 |

## Execution Order

1. Launch O1, O2, and O3 in parallel against the same cleaned benchmark baseline.
2. Require every optimization agent to preserve the current correctness and capture-validation gates.
3. Launch O4 only after O1-O3 have published before/after artifacts.

## Task Packets

### O1: Tile Scheduling And Upload Smoothing
Skill: `terrain-streaming-optimizer`
Goal:
- reduce tile churn and upload burstiness on the heavy overlay case
Deliverables:
- instrumentation for requests, releases, uploads, evictions, and backlog growth
- scheduling or residency changes
- before/after benchmark table for `swiss`
Acceptance:
- `swiss` p95 improves toward `< 25 ms`
- `swiss` p99 improves toward `< 33 ms`
- no correctness metric regression
Prompt:
- Optimize the terrain streaming path in `planetary_terrain_renderer`. Focus on prioritization, hysteresis, cancellation, upload budgeting, and atlas residency. Do not change geodesy or dataset semantics.

### O2: Render-Path Churn Reduction
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
Prompt:
- Optimize the render and extraction path in `planetary_terrain_renderer` using the existing CPU profile as a starting point. Focus on persistent bind groups, reduced full-buffer rewrites, and resource lifetime improvements. Keep benchmark captures validating rendered terrain.

### O3: GPU And Benchmark Attribution Hardening
Skill: `terrain-benchmark-profiler`
Goal:
- turn the benchmark into a better optimization decision tool
Deliverables:
- GPU timing or capture-backed attribution path
- explicit benchmark recipe for throughput and latency sweeps
- refreshed baseline table if measurements materially change
Acceptance:
- optimization work can point to named costly passes or upload phases, not only frame-wide totals
- benchmark captures remain nonblank
Prompt:
- Extend the current benchmark and profiling workflow in `planetary_terrain_renderer` so the next optimization pass has real GPU or pass-level attribution. Keep the benchmark renderer-focused and preserve capture validation.

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
- Swiss renderer benchmark baseline from the capture-validated sweep:
  - FPS mean `44.89`
  - frame mean `22.28 ms`
  - p95 `36.52 ms`
  - p99 `49.81 ms`
  - max `195.47 ms`

## Still Relevant Risks

- missing GPU pass attribution means optimization is still partly guided by inference
- the heaviest overlay case still has visible tail-latency headroom
- local `small_world` coverage for mountainous truth work is still geographically narrow
