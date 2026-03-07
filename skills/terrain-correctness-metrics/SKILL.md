---
name: terrain-correctness-metrics
description: Measure and guard spatial correctness between `small_world` and rendered terrain semantics. Use when adding metrics for renderer ground level, placement error, AGL or MSL alignment, geodesy changes, or validating that coordinate work preserves physical truth.
---

# Terrain Correctness Metrics

Use this skill to turn renderer correctness claims into repeatable measurements.

## Required metrics
- `ground_model_delta_m = renderer_ground_msl - small_world_ground_msl`
- `expected_agl_error_m = -ground_model_delta_m`
- `mapping_delta_m = distance(renderer_native_position, alternative_mapping_position)`

Report each metric with:
- count
- mean abs
- p50 abs
- p95 abs
- max abs
- RMS

## Workflow
1. Keep horizontal placement and vertical frame semantics separate in the analysis.
2. Use renderer-native mapping when comparing against the rendered world.
3. Sample grids, not just single points, in representative terrain.
4. Automate the measurement path so another agent can rerun it unchanged.
5. Add thresholds only for mapping consistency.
   Do not force `ground_model_delta_m` toward zero unless the dataset and datum are actually being unified.

## Rules
- Distinguish model mismatch from dataset or datum mismatch.
- Do not hide large residuals behind screenshots.
- Record exact coordinates, dataset names, interpolation choices, and vertical frames.
- If a metric is too expensive for CI, keep a smoke version in CI and a full version in the benchmarking docs.

## Outputs
- Measurement command or script.
- Current residual table.
- Interpretation of what can and cannot be improved by renderer changes alone.
- Suggested regression gates.

Read [references/hotspots.md](references/hotspots.md) before editing geodesy, placement, or error reporting.
