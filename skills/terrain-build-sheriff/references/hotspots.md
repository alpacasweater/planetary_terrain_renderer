# Build Sheriff Hotspots

## Usual failure points
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/Cargo.toml`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/preprocess/Cargo.toml`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/preprocess/src/split.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/preprocess/src/downsample.rs`

## Baseline commands
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo test -p bevy_terrain_preprocess -- --list`
- `cargo tree -p bevy_terrain_preprocess -i glam@0.29.3`
- `cargo tree -p bevy_terrain_preprocess -i glam@0.30.10`

## Current known blocker
- `bevy_terrain_preprocess` currently mixes `glam 0.29.x` and `glam 0.30.x`, causing `IVec2` type mismatches.
