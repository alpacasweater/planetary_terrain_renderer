---
name: terrain-end-to-end-truth
description: Use when validating end-to-end robot/drone/data placement against the rendered world, building truth matrices, or defining physical-truth merge gates.
---

# Terrain End-To-End Truth

Use this skill when local-frame robot/drone/data positions must land on the rendered world correctly.

## Focus
- local tangent frame placement
- `AGL/MSL/HAE/ECEF` conversion chains
- rendered-surface residuals
- repeatable truth scenarios and merge gates

## Workflow
1. Use renderer-native placement semantics for rendered-world comparisons.
2. Keep vertical conversion truth and horizontal mapping truth separate.
3. Validate grids and paths, not just single points.
4. Save commands and artifact paths for every run.
5. Fail loudly on unexplained residuals.

## Outputs
- truth matrix by region/scenario
- end-to-end placement residuals
- rerunnable smoke/full commands
- pass/fail truth gates

Read these first when needed:
- `docs/correctness_metrics.md`
- `docs/physical_truth_orchestration_plan_2026-03-07.md`
- `scripts/compare_small_world_ground.py`
