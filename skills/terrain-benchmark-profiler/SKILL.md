---
name: terrain-benchmark-profiler
description: Run reproducible benchmarks, captures, and CPU or GPU profiling for `planetary_terrain_renderer`. Use when measuring FPS, frame-time tails, peak memory, upload behavior, or profiling hot paths before and after renderer changes.
---

# Terrain Benchmark Profiler

Use this skill to measure renderer behavior with evidence that another agent can reproduce.

## Workflow
1. Start from a benchmark mode that is representative of the renderer.
   Disable debug-only systems and interaction-only systems unless the task explicitly benchmarks them.
2. Verify that terrain is actually rendered.
   Require nonblank PNG captures or equivalent evidence.
3. Record steady-state and warmup separately.
4. Collect CPU evidence and GPU evidence when possible.
5. Attribute bottlenecks before proposing code changes.

## Required metrics
- FPS mean
- frame time mean, p95, p99, max
- ready or warmup wait
- peak memory footprint
- upload bytes per frame if instrumented
- request or release counts if instrumented
- GPU pass timings if available

## Rules
- Do not use a benchmark example that includes unrelated debug or picking work unless the task says to.
- Keep the exact environment variables and command lines in the handoff.
- Always attach the artifact paths for CSV, JSON, captures, and profiler output.
- Separate present-bound findings from CPU-bound findings.

## Outputs
- Benchmark summary with artifact paths.
- Short attribution summary: CPU-bound, GPU-bound, present-bound, or mixed.
- Before or after comparison if the task is evaluating a change.

Read [references/hotspots.md](references/hotspots.md) before changing benchmark wiring or interpreting profiles.
