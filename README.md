# Planetary Terrain Renderer

A Bevy-based planetary terrain renderer with:
- planetary base terrain plus optional high-resolution overlays
- large-world precision support
- a GDAL-based preprocessing pipeline for georeferenced rasters
- benchmark and correctness tooling for validation work

Project origin:
- [bevy_terrain](https://github.com/kurtkuehnert/bevy_terrain)
- [Master Thesis](https://doi.org/10.60687/2025-0147)

## Quick Start

If you only want to see the renderer working, no extra downloads or native GIS dependencies are required.

```bash
cargo run --example minimal_globe
```

What this gives you:
- a bundled low-resolution Earth under `assets/terrains/earth`
- height plus albedo out of the box
- the smallest copyable example in [examples/minimal_globe.rs](examples/minimal_globe.rs)
- no GDAL setup unless you want to preprocess your own data

Other built-in demos:

```bash
cargo run --example spherical
MULTIRES_OVERLAYS=none cargo run --example spherical_multires
```

## Build Your First Dataset

The simplest preprocess tutorial uses committed sample rasters in `sample_data/`.

1. Install GDAL for preprocessing.
   Use the cross-platform steps in [Getting Started](docs/getting_started.md).
2. Build the tutorial terrain.

```bash
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
```

3. Render the generated dataset with the same minimal example.

```bash
cargo run --example minimal_globe -- terrains/tutorial_earth
```

Notes:
- the render-only path does not require GDAL
- the tutorial preprocess path does not require downloading any raster data
- the full local-source example lives in [preprocess/examples/preprocess_earth.rs](preprocess/examples/preprocess_earth.rs)

## Minimal API

The beginner-friendly surface is intentionally small:
- `TerrainPlugin`: core terrain runtime
- `SimpleTerrainMaterialPlugin`: built-in gradient/albedo material
- `TerrainSettings::with_albedo()`: one-line attachment setup for the common case
- `SimpleTerrainMaterial::for_terrain(...)`: automatically use albedo when present, otherwise fall back to height coloring
- `commands.spawn_terrain(...)`: spawn the terrain once you have a view entity

The example to copy is [examples/minimal_globe.rs](examples/minimal_globe.rs).
It accepts an optional terrain root argument, so the same example works for both bundled and freshly preprocessed data:

```bash
cargo run --example minimal_globe
cargo run --example minimal_globe -- terrains/tutorial_earth
```

## Cross-Platform Preprocessing Setup

Preprocessing is the only part of the project that needs native GIS dependencies.
The recommended setup is documented in [Getting Started](docs/getting_started.md):
- Windows, Linux, and macOS instructions
- a recommended Miniforge/conda-forge path
- the `GDAL_HOME` note for Windows
- zero-download tutorial preprocessing with `sample_data/`

Additional workflow docs:
- [Multi-resolution workflow](docs/multires_workflow.md)
- [Saxony dataset workflow](docs/saxony_workflow.md)

## Saxony Dataset Scripts

```bash
# discover valid URLs
./scripts/download_saxony_dgm1.sh discover

# download discovered ZIPs
./scripts/download_saxony_dgm1.sh download

# extract GeoTIFFs
./scripts/download_saxony_dgm1.sh extract

# build saxony overlay demo assets
./scripts/setup_saxony_partial_demo.sh
```

Helpers:

```bash
./scripts/download_saxony_dgm1.sh status
./scripts/redownload_saxony_dgm1.sh
```

## Performance Benchmarking

Benchmark runner:

```bash
./scripts/benchmark_spherical_multires.sh
```

Details:
- [Performance benchmarking](docs/performance_benchmarking.md)
- [Performance findings (2026-03-07)](docs/performance_findings_2026-03-07.md)

## Correctness Metrics

Ground-model alignment harness:

```bash
python3 scripts/compare_small_world_ground.py \
  --lat 46.55 \
  --lon 10.60 \
  --hgt-root /path/to/hgt_tiles
```

Source-raster parity harness:

```bash
python3 scripts/compare_renderer_to_source_raster.py \
  --lat 46.55 \
  --lon 10.60 \
  --terrain-root assets/terrains/earth \
  --source-raster source_data/gebco_earth_small.tif
```

Multi-region truth matrix:

```bash
python3 scripts/physical_truth_matrix.py \
  --json-out /tmp/physical_truth_matrix.json
```

End-to-end path regression:

```bash
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

Direct geodesy truth tests:

```bash
cargo test math::geodesy::tests:: -- --nocapture
cargo test renderer_wgs84_local_mapping_matches_small_world -- --nocapture
cargo test small_world_ned_orbit_path_maps_to_renderer_local_positions -- --nocapture
```

Details:
- [Correctness metrics](docs/correctness_metrics.md)
- [Physical truth findings (2026-03-07)](docs/physical_truth_findings_2026-03-07.md)
- [Physical truth mapping audit (2026-03-07)](docs/physical_truth_mapping_audit_2026-03-07.md)
- [Physical truth plan (2026-03-07)](docs/physical_truth_orchestration_plan_2026-03-07.md)

Important:
- terrain assets generated before the 2026-03-07 physical-truth mapping fix are stale and should be reprocessed before treating ground/path residuals as authoritative
- for `source_data/gebco_earth_small.tif`, the current physical-truth target for the base Earth height asset is `--lod-count 5`
- for steep regional overlays, do not blindly force a low `--lod-count`; the local `swiss.tif` build requires `lod_count = 9` for acceptable source parity
- the current local `small_world` HGT coverage only overlaps the eastern strip of `swiss.tif`, so the automated Swiss overlay vs `small_world` matrix is intentionally limited to that overlap
- the preprocess CLI currently forces `GDAL_NUM_THREADS=1` because the custom transformer is not yet safely cloneable across GDAL worker threads

## Controls

Camera:
- `T`: toggle fly camera
- `R`: toggle orbital camera
- left click terrain: inspect lat/lon/WGS84 HAE and renderer-local XYZ in the multires demo
- `Cmd+C` or `Ctrl+C`: copy the last clicked terrain inspection result

Visualization:
- `L`: terrain data LOD
- `Y`: geometry LOD
- `Q`: tile tree
- `W`: wireframe

Quality toggles:
- `M`: morphing
- `K`: blending
- `S`: lighting
- `H`: high-precision coordinates

GPU capture (macOS, `metal_capture` feature):
- `C`: capture a frame (`captures/*.gputrace`)

## Current Status

- `cargo check --workspace` and `cargo test --workspace` are green.
- Renderer-native WGS84 and local-frame transforms now agree with `small_world`.
- Base Earth height parity is materially improved with `lod_count = 5`.
- The rebuilt Swiss overlay is physically credible against its source DEM and supports the drone demo.

## Current Plans

- [Optimization and tasking plan](docs/agent-orchestration-plan-2026-03-07.md)
- [Physical truth plan](docs/physical_truth_orchestration_plan_2026-03-07.md)

## License

Dual licensed under:
- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE))

`Thesis.pdf` is excluded from that dual-license and uses CC BY 4.0.
