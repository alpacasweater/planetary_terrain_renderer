# Render Path Optimizer Hotspots

## Key files
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/terrain_data/tile_tree.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/render/terrain_view_bind_group.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/render/terrain_pass.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/src/picking/mod.rs`
- `/Users/biggsba1/Documents/Playground/planetary_terrain_renderer/examples/spherical_multires.rs`

## Current profiler clues
- frequent `Queue::write_buffer*` and staging-buffer creation
- `TileTree::update_terrain_view_buffer` sampled on the hot path
- terrain-view and prepass bind groups are rebuilt every frame
- depth textures are prepared per view in render setup
- benchmark path currently includes picking and debug plugins
