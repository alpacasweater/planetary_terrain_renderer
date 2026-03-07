# Benchmark Profiler Hotspots

## Key files
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/examples/spherical_multires.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/scripts/benchmark_spherical_multires.sh`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/docs/performance_benchmarking.md`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/docs/performance_findings_2026-03-07.md`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/picking/mod.rs`

## Current measured Swiss baseline
From `/tmp/review_profile_run.csv`:
- FPS mean: `44.89`
- frame mean: `22.28 ms`
- p95: `36.52 ms`
- p99: `49.81 ms`
- max: `195.47 ms`

## Current profiler evidence
`/tmp/review_sample.txt` shows:
- frequent `CAMetalLayer nextDrawable` stalls
- repeated `Queue::write_buffer*` activity
- terrain view buffer updates on the hot path
- benchmark example currently includes `TerrainDebugPlugin` and `TerrainPickingPlugin`
