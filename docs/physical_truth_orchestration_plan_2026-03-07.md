# Physical Truth Orchestration Plan (2026-03-07)

## Mission
Make `planetary_terrain_renderer` physically trustworthy for robot, drone, and data placement collected in local tangent frames.

Success means:
- renderer geodesy agrees numerically with `small_world` for shared WGS84 transforms
- preprocess/runtime mapping uses the same geometric semantics
- remaining residual against rendered terrain is explainable by dataset, interpolation, resolution, or vertical datum only

## Phase Status

### Phase 1: Geodesy And Mapping Alignment
Status: complete

Completed:
- direct `small_world` truth tests for `LLA <-> ECEF` and `NED <-> ECEF`
- explicit renderer axis and ellipsoid semantics
- direct mapping acceptance threshold documentation

### Phase 2: Preprocess And Raster Alignment
Status: materially complete

Completed:
- preprocess transform and raster lineage audit
- source raster -> preprocessed tile -> runtime sample tracing
- Earth and Swiss source-parity harnesses
- mapping-version and asset-version implications documented

Open:
- tighter attribution of the remaining Swiss overlay source-parity residual

### Phase 3: End-To-End Truth Regression
Status: partial

Completed:
- multi-region Earth truth matrix
- Swiss overlay overlap-strip truth matrix
- end-to-end path regression for the Swiss overlay
- local-frame orbit regression in unit tests

Open:
- broader mountainous truth coverage beyond the current local HGT overlap
- explicit physical-truth merge gates

## Current Work Packets

### PT1: Expand Truth Coverage
Skill: `terrain-end-to-end-truth`
Goal:
- broaden the physical-truth matrix beyond the currently local HGT overlap tiles
Tasks:
- add more `small_world`-compatible ground coverage for mountainous overlays
- extend `scripts/physical_truth_matrix.py` cases to the new coverage
- rerun the base-earth and overlay suites and publish updated artifacts
Acceptance:
- the truth matrix is no longer dominated by the single Swiss/HGT overlap strip

### PT2: Tighten Overlay Source Parity
Skill: `terrain-raster-truth`
Goal:
- reduce the remaining `~13-20 m` Swiss overlay p95 renderer-vs-source term where it is worth the cost
Tasks:
- profile the remaining Swiss overlay parity error by subregion
- determine whether it is driven by residual face resolution, interpolation, or mask/warp behavior
- document the best size/truth tradeoff for mountainous overlays
Acceptance:
- either the overlay parity improves, or the remaining error is explicitly attributed and bounded

### PT3: Add Physical-Truth Merge Gates
Skill: `terrain-release-verifier`
Goal:
- stop correctness regressions from re-entering the renderer
Tasks:
- define pass/fail thresholds for direct geodesy, Earth truth matrix, Swiss overlay parity, and path regression
- add a concise gate document with exact commands and expected outputs
Acceptance:
- merge readiness can be evaluated from rerunnable commands and concrete thresholds

## Current Hard Truth

- direct renderer `LLA/ECEF/NED` geodesy matches `small_world`
- the dominant renderer-native mapping bug is fixed
- current Earth `lod_count = 5` is close to the dataset floor in flat and coastal regions
- steep-terrain global-base truth is still bounded by source DEM quality and face resolution
- for mountain work, a high-quality regional overlay remains the preferred correctness path
