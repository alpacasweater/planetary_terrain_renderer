# Streaming Optimizer Hotspots

## Key files
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/terrain_data/tile_loader.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/terrain_data/tile_atlas.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/terrain_data/gpu_tile_atlas.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/terrain_data/tile_tree.rs`

## Current structural issues
- loader is LIFO and unprioritized
- stale loads are not cancelled
- atlas cache miss path uses `retain()` scan by atlas index
- uploads are issued one texture write at a time with no frame budget

## Primary success metrics
- Swiss benchmark p95 `< 25 ms`
- Swiss benchmark p99 `< 33 ms`
- Fewer request or release bursts and fewer upload spikes
