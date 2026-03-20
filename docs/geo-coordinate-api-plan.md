# Plan: First-Class Geo Coordinate API

**Date:** 2026-03-10

## Motivation

Downstream users who want to place entities at real-world positions (e.g. a drone at a given
NED offset, a marker at a known LLA) must currently:

1. Manually call `ned_to_ecef` / `lla_hae_to_ecef` from `bevy_terrain::math::geodesy`
2. Manually apply the ECEF → renderer-local axis swap (`DVec3::new(-x, z, y)`) using a
   private function they cannot call
3. Manually call `grid.translation_to_grid(renderer_local)` from `big_space`

None of the geodesy types (`LlaHae`, `Ned`) are in the prelude, `Enu` does not exist, and the
renderer-local conversion layer is entirely private. This makes the crate hostile to anyone
doing real-world geo-referenced entity placement.

## Goal

Make it possible to spawn an entity at a geo-referenced position with a single call, using any
of the four standard coordinate representations:

```rust
use bevy_terrain::prelude::*; // Grid, GridGeoExt, LlaHae, Ned, Enu, CellCoord

let origin = LlaHae { lat_deg: 37.77, lon_deg: -122.42, hae_m: 16.0 };

// From LLA
let (cell, pos) = grid.translation_from_lla(LlaHae { lat_deg: 37.78, lon_deg: -122.41, hae_m: 120.0 });

// From NED (relative to origin)
let (cell, pos) = grid.translation_from_ned(Ned { n_m: 300.0, e_m: 0.0, d_m: -100.0 }, origin);

// From ENU (relative to origin)
let (cell, pos) = grid.translation_from_enu(Enu { e_m: 150.0, n_m: 200.0, u_m: 100.0 }, origin);

// From ECEF
let (cell, pos) = grid.translation_from_ecef(ecef_dvec3);

commands.spawn((CellCoord(cell), Transform::from_translation(pos), ...));
```

## Current State

| Item | Status |
|------|--------|
| `LlaHae` struct | exists in `math::geodesy`, not in prelude |
| `Ned` struct | exists in `math::geodesy`, not in prelude |
| `Enu` struct | **missing** |
| `renderer_local_from_ecef` | exists but **private** |
| `ecef_from_renderer_local` | exists but **private** |
| `*_to_renderer_local` functions | **missing** (users do axis swap by hand) |
| `GridGeoExt` trait | **missing** |
| `CellCoord` in prelude | **missing** |

---

## Implementation Plan

### Phase 1 — Core geodesy (tasks are parallel)

#### T1-A: Add `Enu` type + conversions (`src/math/geodesy.rs`)

- Add `pub struct Enu { pub e_m: f64, pub n_m: f64, pub u_m: f64 }`
  (East-North-Up; U = −D)
- Add `pub fn enu_to_ecef(enu: Enu, origin: LlaHae) -> DVec3`
  — same `GeoConversionParams` rotation as NED, with `u_m = -d_m`
- Add `pub fn ecef_to_enu(ecef: DVec3, origin: LlaHae) -> Enu`

#### T1-B: Expose renderer-local conversion layer (`src/math/geodesy.rs`)

Depends on T1-A for `enu_to_renderer_local`. Treat as sequential with T1-A if done by one person.

- Make `renderer_local_from_ecef` and `ecef_from_renderer_local` `pub`
- Add `pub fn lla_to_renderer_local(lla: LlaHae) -> DVec3`
- Add `pub fn ned_to_renderer_local(ned: Ned, origin: LlaHae) -> DVec3`
- Add `pub fn enu_to_renderer_local(enu: Enu, origin: LlaHae) -> DVec3`
- Add `pub fn ecef_to_renderer_local(ecef: DVec3) -> DVec3` (thin rename of the now-public primitive)

---

### Phase 2 — Grid integration layer (after Phase 1, tasks are parallel)

#### T2-A: Create `src/math/geo_grid.rs` — `GridGeoExt` trait

Extension trait on `big_space::Grid` so callers never touch raw renderer-local DVec3:

```rust
pub trait GridGeoExt {
    fn translation_from_lla(&self, lla: LlaHae) -> (CellCoord, Vec3);
    fn translation_from_ecef(&self, ecef: DVec3) -> (CellCoord, Vec3);
    fn translation_from_ned(&self, ned: Ned, origin: LlaHae) -> (CellCoord, Vec3);
    fn translation_from_enu(&self, enu: Enu, origin: LlaHae) -> (CellCoord, Vec3);
}
```

Each implementation calls the corresponding `*_to_renderer_local` function then
`grid.translation_to_grid(renderer_local)`.

#### T2-B: Update `src/math/mod.rs` exports

- Re-export `LlaHae`, `Ned`, `Enu` from `crate::math`
- Re-export all new `*_to_renderer_local` and `*_from_renderer_local` functions
- Re-export `GridGeoExt` from `crate::math`

---

### Phase 3 — Polish (after Phase 2, tasks are parallel)

#### T3-A: Update prelude (`src/lib.rs`)

Add to `prelude`:
- `LlaHae`, `Ned`, `Enu` (from `crate::math`)
- `GridGeoExt` (from `crate::math`)
- `CellCoord` (from `big_space::prelude`) — currently missing, required for entity bundles

The lower-level `*_to_renderer_local` functions remain accessible via
`bevy_terrain::math::geodesy` without cluttering the prelude.

#### T3-B: Add tests (`src/math/geodesy.rs` test module)

- ENU round-trip: `enu_to_ecef` → `ecef_to_enu` within 1e-6 m
- ENU vs `small_world`: `enu_to_ecef` matches `small_world::Enu::to_ecef` to sub-millimeter
- `lla_to_renderer_local` / `ned_to_renderer_local` / `enu_to_renderer_local` round-trip
  through `renderer_local_to_lla_hae`
- `GridGeoExt` integration: all four methods produce identical results for equivalent points

#### T3-C: Update `drone_mission_viz` (reference downstream consumer)

- Delete `src/geo_bridge.rs` — was a workaround for the missing public API
- Replace all `ned_to_renderer_local` call sites in `markers.rs`, `path_vis.rs`, `camera.rs`,
  `scene.rs` with `grid.translation_from_ned(Ned { ... }, origin)` via `GridGeoExt`
- Remove `mod geo_bridge` from `main.rs`

---

## Dependency Graph

```
T1-A ─┐
       ├─> T1-B ─┬─> T2-A ─┬─> T3-A
                 │           ├─> T3-B
                 └─> T2-B ──┤
                             └─> T3-C
```

T3-A, T3-B, and T3-C are fully parallel once T2-A and T2-B are complete.
