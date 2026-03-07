# Planetary Terrain Renderer

A Bevy-based planetary terrain renderer with:
- global base terrain + local high-resolution overlays
- large-world precision support
- preprocessing pipeline for georeferenced rasters
- benchmarking and visual capture tooling

Project origin:
- [bevy_terrain](https://github.com/kurtkuehnert/bevy_terrain)
- [Master Thesis](https://doi.org/10.60687/2025-0147)

## Quick Start

From repo root:

```bash
cargo run --example spherical
```

Multi-resolution demo (base + overlays):

```bash
cargo run --example spherical_multires
```

Choose overlays:

```bash
# base only
MULTIRES_OVERLAYS=none cargo run --example spherical_multires

# swiss, saxony, los, srtm_* are supported keys
MULTIRES_OVERLAYS=swiss,los cargo run --example spherical_multires
```

## Data and Preprocessing

Use the preprocess CLI (or `preprocess/examples`) to convert GeoTIFF inputs into
renderer assets under `assets/terrains/*`.

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

Latest measured findings and optimization plan:
- [Performance findings (2026-03-07)](docs/performance_findings_2026-03-07.md)

## Controls (Debug)

Camera:
- `T`: toggle fly camera
- `R`: toggle orbital camera

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

## License

Dual licensed under:
- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE))

`Thesis.pdf` is excluded from that dual-license and uses CC BY 4.0.
