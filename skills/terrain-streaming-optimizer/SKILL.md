---
name: terrain-streaming-optimizer
description: Reduce tile churn, load thrash, upload bursts, and atlas residency misses in `planetary_terrain_renderer`. Use when profiling points to tile scheduling, loading, atlas eviction, or attachment upload behavior as a source of latency spikes.
---

# Terrain Streaming Optimizer

Use this skill to improve frame pacing by fixing the terrain streaming path without changing geodesy or visual truth semantics.

## Workflow
1. Instrument the tile lifecycle first.
   Count requests, releases, in-flight loads, completed loads, cancellations, atlas evictions, and uploaded bytes.
2. Fix scheduling before adding more concurrency.
   Prioritization, hysteresis, and cancellation are higher value than just increasing parallelism.
3. Reduce burstiness.
   Add per-frame budgets for uploads and request issuance.
4. Remove avoidable algorithmic costs.
   Prefer direct atlas-index bookkeeping over whole-map scans.
5. Re-benchmark on heavy overlay scenarios.

## Rules
- Do not change coordinate math in this task.
- Prove that latency tails improve on the heavy benchmark scenario, not just the easy one.
- Keep the loader behavior deterministic enough to compare before and after runs.

## Outputs
- Instrumentation summary.
- Code changes to the streaming path.
- Before or after benchmark table focused on p95, p99, and max.

Read [references/hotspots.md](references/hotspots.md) before changing tile scheduling or atlas behavior.
