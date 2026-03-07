# Correctness Metrics Hotspots

## Key files
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/math/geodesy.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/math/coordinate.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/preprocess/src/transformers.rs`
- `/Users/biggsba1/Documents/Playground/planetary_test/src/main.rs`
- `/Users/biggsba1/Documents/Playground/planetary_test/scripts/compare_ground_models.py`
- `/Users/biggsba1/Documents/Playground/planetary_test/docs/ground_level_alignment_plan.md`

## Current measured baseline
At `lat=46.55`, `lon=10.60` on an `81` point grid:
- center delta: `-253.25 m`
- mean abs delta: `159.47 m`
- p95 abs delta: `380.85 m`
- max abs delta: `481.99 m`
- RMS delta: `195.23 m`

## Immediate targets
- `mapping_delta_m` p95 `< 1 m`
- `mapping_delta_m` max `< 5 m`
- `ground_model_delta_m` tracked and explained, not hand-waved away
