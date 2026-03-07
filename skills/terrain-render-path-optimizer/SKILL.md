---
name: terrain-render-path-optimizer
description: Reduce per-frame buffer churn, bind-group churn, depth-resource churn, and render-pass overhead in `planetary_terrain_renderer`. Use when profiles show `Queue::write_buffer*`, resource recreation, or terrain pass overhead on the hot path.
---

# Terrain Render Path Optimizer

Use this skill to reduce frame cost in the render and extraction path while preserving renderer behavior.

## Workflow
1. Identify per-frame allocations, writes, and bind-group rebuilds from the profiler.
2. Make resource lifetime explicit.
   Recreate GPU resources only when inputs actually change.
3. Replace full-buffer rewrites with dirty or partial updates where practical.
4. Keep benchmark mode clean enough that renderer improvements are measurable.
5. Re-run benchmark and profile after each substantial change.

## Rules
- Do not mix rendering-path work with dataset or geodesy changes.
- If a change alters benchmark wiring, keep a benchmark-only path and preserve interactive behavior separately.
- Validate with captures after resource-lifetime changes.

## Outputs
- Hot-path attribution before and after.
- Code changes to persistent resources or reduced write traffic.
- Benchmark comparison centered on p95, p99, and peak memory.

Read [references/hotspots.md](references/hotspots.md) before changing render-pass or bind-group code.
