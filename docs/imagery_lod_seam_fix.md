# Imagery LOD Seam Fix (Spherical Terrain)

## Problem Statement

During spherical rendering, imagery looked correct at the coarsest level (LOD 1), but clear cross-tile artifacts appeared when LOD 2 became visible. Reported examples included:

- Italy/Greece/Mediterranean appearing between top and bottom Africa tiles.
- Great Lakes content appearing between top and bottom South America tiles.

This indicated a runtime tile lookup/positioning error rather than a preprocessing content error.

## Findings

1. Preprocessed albedo tiles were internally consistent.
- LOD atlas and per-LOD map-layout validation looked correct when rendered as flat world maps.
- Artifacts only appeared in runtime spherical sampling at finer LOD transitions.

2. Root cause was in LOD coordinate remapping across cube-face boundaries.
- The old `coordinate_change_lod` implementation rescaled local `xy` and `uv` directly.
- That local-only approach does not preserve the same world-space point when crossing face boundaries.
- Result: blend/lookup could select a neighbor tile from a different logical region, producing continent "leaks" across seams.

3. A secondary robustness issue existed in blend LOD casting.
- `target_lod` (float) was cast to `u32` before clamping in all cases.
- Negative values can produce invalid integer behavior on cast boundaries.

## Fixes Applied

All production fixes are in:

- [`src/shaders/functions.wgsl`](/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/shaders/functions.wgsl)

### 1) Canonical world-space LOD remap in `coordinate_change_lod`

Added:

- `FaceUv` helper struct
- `coordinate_to_unit_position(coordinate)`
- `coordinate_from_unit_position(unit_position)`

Then rewrote `coordinate_change_lod` to:

1. Convert current `Coordinate` to canonical unit-space position.
2. Recompute canonical `(face, uv)` from that unit-space position.
3. Rebuild target `(lod, xy, uv)` from canonical UV at `new_lod`.

This guarantees LOD changes preserve geographic position, including at face borders.

### 2) Fragment derivative safety for face changes

Inside `coordinate_change_lod` (`#ifdef FRAGMENT` path):

- Scale `uv_dx/uv_dy` only if face is unchanged.
- Reset derivatives to zero when face changes.

This avoids using stale derivatives from a different face parameterization.

### 3) Blend LOD clamping before integer conversion

In `compute_blend`:

- Clamp `target_lod` to `[0, lod_count - 1]` as float.
- Convert clamped value to `u32`.
- Use explicit `blend_enabled` condition for ratio gating.

This removes potential negative/overflow cast behavior and keeps blend-stage lookup bounded.

## Validation Ladder Used

1. Preprocess output verification
- Checked map-layout artifacts for each LOD.
- Confirmed source tiles were not the origin of the seam artifacts.

2. Runtime stage isolation
- Compared geometry LOD, data LOD, and blend-stage lookups.
- Confirmed mismatch emerged during LOD conversion/lookup, not during basic texturing.

3. LOD roundtrip verification
- Verified `geometry -> blend LOD -> geometry LOD` mapping stability.
- After fix, major cross-seam mismatches disappeared.

4. Heightmap parity check
- Repeated alignment checks in height-focused views.
- No comparable structural seam bug remained after the same coordinate fix.

## Cleanup Performed

Removed all temporary debug instrumentation and ad hoc validation artifacts from code so only actual fixes remain.

Reverted debug edits in:

- `assets/shaders/spherical.wgsl`
- `examples/spherical.rs`
- `src/debug/mod.rs`
- `src/render/terrain_material.rs`
- `src/shaders/debug.wgsl`
- `src/shaders/render/fragment.wgsl`

Removed temporary local helper scripts:

- `scripts/` (untracked debug utilities)

## Current Change Scope

Production code change:

- [`src/shaders/functions.wgsl`](/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/shaders/functions.wgsl)

Documentation:

- [`docs/imagery_lod_seam_fix.md`](/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/docs/imagery_lod_seam_fix.md)

## Build Verification

Ran:

- `cargo check --example spherical`

Result:

- Success (existing non-fatal warnings remain, unrelated to this fix).
