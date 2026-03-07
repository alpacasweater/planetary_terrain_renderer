---
name: terrain-geodesy-truth
description: Use when comparing renderer geodesy against `small_world`, defining axis/frame conventions, validating WGS84 LLA/ECEF/NED transforms, or documenting ellipsoid-vs-sphere semantics.
---

# Terrain Geodesy Truth

Use this skill when physical truth depends on exact agreement between renderer math and `small_world` WGS84 transforms.

## Focus
- `LLA <-> ECEF`
- `NED <-> ECEF`
- renderer local-axis relationship to WGS84 ECEF
- geodetic normal vs ECEF radial distinctions

## Workflow
1. Start with direct numerical comparisons against `small_world`.
2. Separate transform correctness from terrain dataset correctness.
3. Make axis conventions explicit with one tested mapping, not prose alone.
4. Treat ellipsoid-vs-sphere assumptions as first-class correctness issues.
5. Leave behind exact tolerances and commands.

## Outputs
- direct truth tests or harnesses
- explicit renderer<->ECEF axis mapping
- documented invariants and non-invariants
- acceptance thresholds for geodesy changes

Read these first when needed:
- `src/math/geodesy.rs`
- `src/math/coordinate.rs`
- `src/math/terrain_shape.rs`
- `docs/physical_truth_orchestration_plan_2026-03-07.md`
