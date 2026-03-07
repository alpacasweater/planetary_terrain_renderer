# Physical Truth Findings (2026-03-07)

## What Is Solved

- direct renderer `LLA/ECEF/NED` geodesy matches `small_world`
- renderer-native WGS84 local placement matches `small_world`
- preprocess and runtime mapping semantics have been aligned
- stale Earth and Swiss terrain assets have been regenerated locally

Direct truth checks live in:
- `src/math/geodesy.rs`

## What Was Fixed

1. geodetic `lat/lon` no longer goes through a spherical shortcut
2. preprocess cube-face lon/lat transforms now use the same corrected mapping as runtime helpers
3. CPU ellipsoid height offsets now use the ellipsoid normal instead of the radial direction

Implementation details and root-cause audit:
- `docs/physical_truth_mapping_audit_2026-03-07.md`

## Earth Base Status

The Earth height dataset was regenerated from:
- `source_data/gebco_earth_small.tif`

Current local Earth config:
- `format_version = 2`
- `geodetic_mapping_version = 2`
- `lod_count = 5`

Key result near `lat=46.55, lon=10.60`:
- renderer vs source center error improved from `-155.833 m` to `-1.243 m`
- renderer vs source p95 abs improved from `216.856 m` to `55.948 m`

Interpretation:
- the direct geodetic frame mismatch is fixed
- the old coarse Earth height build was a major source of renderer-side error
- the remaining Alps mismatch is now mostly GEBCO-vs-HGT dataset disagreement rather than renderer mapping drift

## Multi-Region Truth Matrix

Measured with `scripts/physical_truth_matrix.py`:

- `alps_peak`
  - source-vs-`small_world` p95 abs: `275.228 m`
  - renderer-vs-`small_world` p95 abs: `297.754 m`
  - renderer-vs-source p95 abs: `55.948 m`
- `alps_west_slope`
  - source-vs-`small_world` p95 abs: `436.387 m`
  - renderer-vs-`small_world` p95 abs: `435.214 m`
  - renderer-vs-source p95 abs: `41.060 m`
- `alps_east_slope`
  - source-vs-`small_world` p95 abs: `458.344 m`
  - renderer-vs-`small_world` p95 abs: `467.051 m`
  - renderer-vs-source p95 abs: `5.642 m`
- `florida_keys`
  - source-vs-`small_world` p95 abs: `1.844 m`
  - renderer-vs-`small_world` p95 abs: `1.782 m`
  - renderer-vs-source p95 abs: `0.106 m`
- `florida_lower_keys`
  - source-vs-`small_world` p95 abs: `3.952 m`
  - renderer-vs-`small_world` p95 abs: `3.943 m`
  - renderer-vs-source p95 abs: `0.445 m`
- `florida_north_tile`
  - source-vs-`small_world` p95 abs: `0.587 m`
  - renderer-vs-`small_world` p95 abs: `0.595 m`
  - renderer-vs-source p95 abs: `0.075 m`

Interpretation:
- low-relief coastal cases are effectively at dataset floor
- steep-relief Alpine cases are no longer dominated by the old renderer mapping bug
- some steep-relief subregions still retain `~5-56 m` p95 renderer error above the source-vs-`small_world` floor

## LOD6 Global-Base Experiment

A full-Earth `lod_count = 6` experiment helped Alpine renderer-above-floor residuals, but not enough to justify its storage cost as the default strategy.

Storage comparison:
- official `lod_count = 5` Earth height asset: about `2.4G`
- experimental `lod_count = 6` Earth height asset: about `8.0G`

Current recommendation:
- keep the base Earth height asset at `lod_count = 5`
- use higher-quality regional overlays wherever steep-terrain physical truth matters

## Swiss Overlay Fidelity Audit

The Swiss regional overlay (`source_data/swiss.tif`, native `80 m`) was rebuilt to the preprocess heuristic target (`lod_count = 9`).

Measured source-raster parity:
- Bernese Oberland: `9.8 m RMS`, `22.0 m p95`
- Central Switzerland: `9.5 m RMS`, `21.7 m p95`
- Engadin: `10.1 m RMS`, `17.7 m p95`

Operational finding:
- high-LOD overlay builds originally crashed inside `libproj`
- safe interim fix: disable similar-transformer cloning and force `GDAL_NUM_THREADS=1`

## Swiss Overlay Truth Matrix In The Local HGT Overlap

Measured with:

```bash
python3 scripts/physical_truth_matrix.py \
  --suite swiss_overlay \
  --hgt-root /Users/biggsba1/Documents/Playground/planetary_test/data/srtm \
  --json-out /tmp/physical_truth_matrix_swiss_overlay.json
```

Results:
- `swiss_border_south`: renderer vs source p95 `13.273 m`, renderer vs `small_world` p95 `29.303 m`
- `swiss_border_high_relief`: renderer vs source p95 `19.646 m`, renderer vs `small_world` p95 `40.210 m`
- `swiss_border_north`: renderer vs source p95 `15.678 m`, renderer vs `small_world` p95 `35.897 m`

Interpretation:
- the rebuilt Swiss overlay is in the `~13-20 m` p95 class above its source raster in the HGT overlap strip
- the remaining renderer-vs-`small_world` gap there is mostly dataset floor, not renderer frame error

## End-To-End Overlay Path Regression

Measured with a `1 km`, `100 m AGL`, `64`-sample orbit around `46.70, 10.40`:
- source-raster anchored path: `13.240 m` p95 rendered AGL error, `16.605 m` max
- `small_world` anchored path: `40.314 m` p95 rendered AGL error, `75.148 m` max

Interpretation:
- source-anchored path placement over the rebuilt Swiss overlay is physically credible
- `small_world`-anchored path placement in this strip is still limited by the HGT-vs-DEM floor, not by the renderer-local geodesy path

## Current Physical-Truth Status

What remains open:
- local `small_world` HGT coverage is too narrow for broad mountainous validation
- steep-terrain global-base truth is still limited by base DEM quality and face resolution
- the Swiss overlay is good enough for a physically credible drone demo, but not yet at sub-10 m p95 everywhere against its source raster

Practical consequence:
- for truth-critical mountain work, the current best path is a high-quality regional overlay plus source-raster-anchored validation, not a higher-cost global-base-only strategy
