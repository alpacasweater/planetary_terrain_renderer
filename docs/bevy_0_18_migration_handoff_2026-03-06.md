# Bevy 0.18 Migration Handoff (2026-03-06)

This document captures the exact state of the Bevy 0.18 migration work so a new
thread can continue without rediscovery.

## Status Update (Later Same Day)

- `cargo check`: passes
- `cargo test`: passes (examples compile)
- Runtime smoke test: `cargo run --example spherical` starts successfully
- Remaining warning: `unexpected cfg condition value: webgpu` emitted from
  `#[derive(AsBindGroup)]` in `src/render/terrain_bind_group.rs` (non-blocking)

## Snapshot

- Repo: `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer`
- Branch: `codex/touchpad-camera-controls`
- Base commit: `1f46837` (`Merge branch 'codex/imagery-lod-seam-fix'`)
- Working tree: dirty (17 modified files from migration work, plus this untracked handoff doc)
- Dependency bump already applied in `Cargo.toml`:
  - `bevy = "0.18.1"`
  - `bevy_common_assets = { version = "0.15.0", features = ["ron"] }`
  - `big_space = { version = "0.12.0", features = ["i32"] }`

Modified files:

- `Cargo.toml`
- `src/debug/camera.rs`
- `src/debug/metal_capture.rs`
- `src/debug/mod.rs`
- `src/debug/orbital_camera.rs`
- `src/formats/tiff.rs`
- `src/picking/mod.rs`
- `src/plugin.rs`
- `src/preprocess/mipmap.rs`
- `src/render/terrain_bind_group.rs`
- `src/render/terrain_material.rs`
- `src/render/terrain_pass.rs`
- `src/render/terrain_view_bind_group.rs`
- `src/render/tiling_prepass.rs`
- `src/terrain.rs`
- `src/terrain_data/tile_atlas.rs`
- `src/terrain_data/tile_tree.rs`

## Current Build Status

Latest run:

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
cargo check
```

Result: fails with 35 errors (+1 warning).

## What Is Already Done

- Initial Bevy 0.18 dependency bump completed.
- Most API renames from older render scheduling completed:
  - `RenderSet` -> `RenderSystems` in several files.
  - `RenderGraphApp` usage moved to `RenderGraphExt`.
- `HookContext` import migrated to `bevy::ecs::lifecycle::HookContext`.
- Observer API partly migrated (`Trigger<ReadbackComplete>` -> `On<ReadbackComplete>` in some paths).
- `GridCell` usage migrated in several places to `CellCoord`.
- Some lifetime alias updates for `QueryItem`/`ROQueryItem` completed.
- Fullscreen shader access updated in terrain pass (`FullscreenShader`).
- `RenderAssetUsages` import moved for TIFF loader.
- Window cursor handling in debug camera/orbital camera partially migrated to `CursorOptions`.

## Blocking Errors (Grouped)

### 1) `ron` serde decode/encode currently wrong (`src/terrain.rs`)

Current code imports `bevy_common_assets::ron` and calls:

- `ron::from_str(...)`
- `ron::ser::to_string_pretty(...)`

These symbols are missing in that namespace.

### 2) Transform system enum rename not completed

Still using old symbol:

- `TransformSystem::TransformPropagate`

in:

- `src/plugin.rs`
- `src/picking/mod.rs`

Should use current `TransformSystems` variant(s).

### 3) Math/handle API renames not finished

Remaining old calls:

- `.compute_matrix()` (needs `.to_matrix()`) in:
  - `src/picking/mod.rs`
  - `src/terrain_data/tile_tree.rs`
- `.clone_weak()` (needs `.clone()`) in:
  - `src/picking/mod.rs`
  - `src/terrain_data/tile_tree.rs`

### 4) Ambient light API changed

`AmbientLight` is no longer inserted as a resource in Bevy 0.18.

- Failing code in `src/debug/mod.rs` (`commands.insert_resource(AmbientLight { ... })`).

### 5) Render pipeline descriptor API changed (major)

Across render/compute pipeline creation, code still passes:

- `Vec<BindGroupLayout>` where Bevy 0.18 expects `Vec<BindGroupLayoutDescriptor>`
- `entry_point: "...".into()` where Bevy 0.18 expects `Option<Cow<'static, str>>`

Affected files:

- `src/picking/mod.rs`
- `src/preprocess/mipmap.rs`
- `src/render/terrain_material.rs`
- `src/render/terrain_pass.rs`
- `src/render/tiling_prepass.rs`

### 6) `AsBindGroup::as_bind_group` signature changed (major)

Now requires:

- `&BindGroupLayoutDescriptor`
- `&RenderDevice`
- `&PipelineCache`
- params

Current calls in `src/render/terrain_view_bind_group.rs` still pass old args and `BindGroupLayout`.

### 7) Mesh pipeline view layout type changed

In `src/render/terrain_material.rs`, `mesh_pipeline.get_view_layout(...)` returns `MeshPipelineViewLayout`, not `BindGroupLayout`.

- Need to use appropriate descriptor fields (e.g., `main_layout`) when building pipeline layouts.

### 8) Material rendering glue issues

In `src/render/terrain_material.rs`:

- `ShaderRef` type is currently unresolved (missing import/path for Bevy 0.18).
- `type DrawTerrain<M> = ...` now errors as unused type parameter.

## Recommended Resume Order

1. **Low-risk API fixes first**
   - Fix `ron` parse/serialize imports in `src/terrain.rs`.
   - Replace `TransformSystem` old usages.
   - Replace `.compute_matrix()` and `.clone_weak()` usages.
   - Fix ambient-light setup in `src/debug/mod.rs`.

2. **Descriptor migration foundation**
   - Standardize pipeline creation to use `BindGroupLayoutDescriptor` and `entry_point: Some("...".into())`.
   - Apply consistently to picking, mipmap, terrain material, terrain pass, tiling prepass.

3. **`AsBindGroup` migration**
   - Thread `PipelineCache` into bind group build systems.
   - Convert stored layout types/fields to descriptor-based usage where required.

4. **Terrain material specialization cleanup**
   - Fix `ShaderRef` import/path.
   - Update view layout handling (`MeshPipelineViewLayout` descriptors).
   - Remove/fix unused generic param in `DrawTerrain<M>` alias and callsites.

5. **Re-check and run examples**
   - `cargo check`
   - `cargo test` (if applicable)
   - `cargo run --example spherical` (or current demo target)

## Useful Commands for Next Thread

```bash
cd /Users/biggsba1/Documents/Playground/planetary_terrain_renderer
git checkout codex/touchpad-camera-controls
git status --short --branch
cargo check
rg -n "TransformSystem::TransformPropagate|compute_matrix\(|clone_weak\(|entry_point: \"|layout: vec!\[" src
```

## Notes

- No destructive cleanup was performed.
- No migration commit has been made yet for this WIP.
- Current state is safe to continue from directly on this branch.
