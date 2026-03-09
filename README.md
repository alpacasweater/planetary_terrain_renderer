# Planetary Terrain Renderer

A Bevy-based planetary terrain renderer with:
- global base terrain plus local high-resolution overlays
- large-world precision support
- preprocessing pipeline for georeferenced rasters
- benchmark and visual-capture tooling
- physical-truth evaluation against `small_world`

Project origin:
- [bevy_terrain](https://github.com/kurtkuehnert/bevy_terrain)
- [Master Thesis](https://doi.org/10.60687/2025-0147)

## Quick Start

A low-resolution Earth starter dataset is included in the repo.

Run the globe demo:

```bash
cargo run --example spherical
```

Optional base-only multi-resolution demo:

```bash
MULTIRES_OVERLAYS=none cargo run --example spherical_multires
```

Notes:
- the starter Earth lives under `assets/terrains/earth` and includes both height and albedo
- it is intentionally low resolution so a clean clone stays small and the examples start immediately
- `./scripts/setup_earth_quickstart.sh` rebuilds a starter-sized Earth locally and adds albedo when `source_data/true_marble.tif` is available
- for the larger validated Earth build and regional overlays, use [Multi-resolution workflow](docs/multires_workflow.md)

Once you have built additional overlay datasets, you can load them in the multi-resolution demo:

```bash
MULTIRES_OVERLAYS=swiss,los cargo run --example spherical_multires
```

Swiss drone demo:

```bash
MULTIRES_OVERLAYS=swiss \
MULTIRES_ENABLE_DRONE=1 \
MULTIRES_DRONE_AGL_M=250 \
MULTIRES_DRONE_ORBIT_RADIUS_M=1500 \
cargo run --example spherical_multires
```

Swiss click inspection demo:

```bash
MULTIRES_OVERLAYS=swiss \
MULTIRES_ENABLE_CLICK_READOUT=1 \
cargo run --example spherical_multires
```

## Current Status

- `cargo check --workspace` and `cargo test --workspace` are green.
- Renderer-native WGS84 and local-frame transforms now agree with `small_world`.
- Base Earth height parity is materially improved with `lod_count = 5`.
- The rebuilt Swiss overlay is physically credible against its source DEM and supports the drone demo.
- Current validated Swiss low-latency benchmark baseline is about `103.76 FPS`, `9.64 ms` mean frame time, `14.39 ms` p95, and `17.31 ms` p99.
- The main open work is performance: remaining GPU/pass attribution, terrain depth-copy or main-pass cost on the low-latency baseline, and cleanup of noisy missing-asset paths. The old CPU upload hotspot is no longer the primary accepted target on the `MSAA=1` Swiss benchmark path.

## Data and Preprocessing

Use the preprocess CLI (or `preprocess/examples`) to convert GeoTIFF inputs into renderer assets under `assets/terrains/*`.

Primary workflow docs:
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

Detailed benchmark usage and env vars:
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

## Controls (Debug)

Camera:
- `T`: toggle fly camera
- `R`: toggle orbital camera
- left click terrain: inspect lat/lon/WGS84 HAE and renderer-local XYZ in the multires demo
- `Cmd+C` or `Ctrl+C`: copy the last clicked terrain inspection result

Visualization:
- `L` terrain data LOD
- `Y` geometry LOD
- `Q` tile tree
- `W` wireframe

Quality toggles:
- `M` morphing
- `K` blending
- `S` lighting
- `H` high-precision coordinates

GPU capture (macOS, `metal_capture` feature):
- `C`: capture a frame (`captures/*.gputrace`)

## Current Plans

- [Optimization and tasking plan](docs/agent-orchestration-plan-2026-03-07.md)
- [Physical truth plan](docs/physical_truth_orchestration_plan_2026-03-07.md)

## License

Dual licensed under:
- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE))

`Thesis.pdf` is excluded from that dual-license and uses CC BY 4.0.
