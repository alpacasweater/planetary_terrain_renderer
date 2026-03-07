---
name: terrain-raster-truth
description: Use when auditing preprocess/runtime sample parity, GDAL transforms, face projection semantics, interpolation, or raster provenance for physical-truth work.
---

# Terrain Raster Truth

Use this skill when the question is whether source raster values survive preprocessing and runtime sampling with the same world meaning.

## Focus
- GDAL warp/sample semantics
- pixel-center and interpolation choices
- face projection and tile addressing
- source raster -> preprocessed tile -> runtime sample lineage

## Workflow
1. Trace one value end-to-end before making broad claims.
2. Keep datum, interpolation, and geometry mismatches separated.
3. Prefer fixed lat/lon truth samples over screenshots.
4. Record source files, commands, and interpolation mode every time.
5. Avoid changing runtime mapping and preprocess mapping independently.

## Outputs
- sample-lineage trace
- preprocess/runtime parity checks
- documented attributable residuals
- rebuild/versioning implications if semantics change

Read these first when needed:
- `preprocess/src/`
- `scripts/compare_small_world_ground.py`
- `docs/correctness_metrics.md`
- `docs/physical_truth_orchestration_plan_2026-03-07.md`
