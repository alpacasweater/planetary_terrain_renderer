# Correctness Metrics

This repo includes ground, raster, matrix, and path-level correctness harnesses.

## Ground Alignment Harness

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
python3 scripts/compare_small_world_ground.py \
  --lat 46.55 \
  --lon 10.60 \
  --hgt-root /path/to/hgt_tiles \
  --json-out /tmp/ground_metrics.json
```

Measures:
- `ground_model_delta_m = renderer_ground_msl - small_world_ground_msl`
- `expected_agl_error_m = -ground_model_delta_m`

`expected_agl_error_m` is the practical rendering error if an object or path altitude is derived from `small_world` ground truth but placed against the currently rendered terrain surface.

## Source Raster Parity Harness

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
python3 scripts/compare_renderer_to_source_raster.py \
  --lat 46.55 \
  --lon 10.60 \
  --terrain-root assets/terrains/earth \
  --source-raster source_data/gebco_earth_small.tif \
  --json-out /tmp/renderer_to_source.json
```

Measures:
- `preprocess_runtime_delta_m = renderer_ground_msl - source_raster_ground_msl`

This isolates renderer/preprocess distortion from disagreement between the source dataset and an external truth model such as `small_world`.

## Physical Truth Matrix

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
python3 scripts/physical_truth_matrix.py \
  --json-out /tmp/physical_truth_matrix.json
```

Current suites:
- default Earth base suite
- `swiss_overlay` suite for the local HGT-overlap strip

The matrix reports:
- `renderer_vs_small_world`
- `source_vs_small_world`
- `renderer_vs_source`

`renderer_vs_source` is the renderer residual above the source-vs-`small_world` dataset floor.

## End-To-End Path Metric

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
python3 scripts/path_truth_regression.py \
  --origin-lat 46.70 \
  --origin-lon 10.40 \
  --radius-m 1000 \
  --commanded-agl-m 100 \
  --sample-count 64 \
  --truth-ground source_raster \
  --terrain-root assets/terrains/swiss_highres \
  --source-raster source_data/swiss.tif \
  --json-out /tmp/swiss_overlay_path_truth_source.json
```

This turns terrain residuals into the operational metric that matters for drone and robot overlays: rendered AGL error along a path.

Current Swiss overlay result in the HGT-overlap strip:
- source-raster anchored path: `13.240 m` p95 rendered AGL error
- `small_world` anchored path: `40.314 m` p95 rendered AGL error

Interpretation:
- source-anchored placement over the rebuilt Swiss overlay is physically credible
- the remaining `small_world`-anchored error is mostly DEM-vs-HGT floor, not renderer frame error

## Current Earth Baseline

For `source_data/gebco_earth_small.tif`, the local `assets/terrains/earth` height pyramid is built with `--lod-count 5`.

At `lat=46.55, lon=10.60` over an `81`-point `2 km x 2 km` grid:
- renderer vs source raster, old `lod_count = 4`: center `-155.833 m`, p95 abs `216.856 m`
- renderer vs source raster, current `lod_count = 5`: center `-1.243 m`, p95 abs `55.948 m`

## Current Multi-Region Interpretation

With the current Earth height build (`lod_count = 5`):
- flat and coastal cases are effectively at dataset floor
- steep-relief Alpine cases are much better than before, but still retain tens of meters of p95 renderer error above the source-vs-`small_world` floor in some subregions

Swiss overlay in the local HGT-overlap strip:
- renderer vs source p95: `13-20 m`
- renderer vs `small_world` p95: `29-40 m`

Interpretation:
- renderer geodesy and mapping are no longer the dominant blocker
- local overlay quality and source-dataset disagreement now dominate the remaining error budget in mountainous truth-critical regions

## Requirements

- renderer terrain assets at `assets/terrains/*`
- `small_world`-compatible HGT tiles for `small_world` comparisons
- `gdallocationinfo` on `PATH`

## Operational Notes

- the harnesses use the corrected renderer geodetic mapping
- terrain assets generated before the 2026-03-07 physical-truth mapping fix are stale and should be rebuilt before treating results as authoritative
- the local Swiss overlay requires `lod_count = 9` class resolution for acceptable source parity
- the preprocess CLI currently forces `GDAL_NUM_THREADS=1` because the custom transformer is not yet safely cloneable across GDAL worker threads
