# Getting Started

This project has two distinct paths:
- render-only: no native GIS dependencies, just `cargo run --example minimal_globe`
- preprocessing: requires GDAL so you can convert GeoTIFFs and other GDAL-readable rasters into terrain assets

If you are evaluating the renderer for the first time, start with the render-only path.

## 1. Render Something Immediately

The repo already includes a starter Earth dataset.

```bash
cargo run --example minimal_globe
cargo run --example minimal_globe -- --max-lod 7
OPENTOPOGRAPHY_API_KEY=your-key cargo run --example minimal_globe -- --max-lod 7 --stream-height
```

This launches the smallest demo intended for copying into your own app.
The source is [examples/minimal_globe.rs](../examples/minimal_globe.rs).
If you later add a streamed cache, keep the same example and set `TERRAIN_STREAMING_CACHE_ROOT=streaming_cache` to prefer cached tiles before the bundled starter data.

Default minimal example behavior:
- target max LOD defaults to `7`
- use `--max-lod N` or `MINIMAL_GLOBE_MAX_LOD=N` to change it
- use `--stream-height` when you want real OpenTopography DEM refinement instead of the bundled coarse starter height
- if you request a higher LOD than the local terrain asset contains and do not provide cached tiles or `--stream-height`, the renderer falls back to the coarser local terrain

If you want the example to fill an imagery cache from the network, opt in explicitly:

```bash
cargo run --example minimal_globe -- --stream-online
```

This keeps the runtime model simple:
- bundled starter Earth renders immediately, even offline
- missing `albedo` tiles can be fetched online and written under `assets/streaming_cache/`
- later runs reuse the warmed cache before falling back to the bundled Earth
- `minimal_globe` targets `lod 7` by default; use `--max-lod N` or `MINIMAL_GLOBE_MAX_LOD=N` if you want a different cap

Current online limits:
- the current provider is NASA GIBS true-color imagery
- requests that cross the antimeridian are not implemented yet

If you also want online height refinement, opt in separately and provide an OpenTopography API key:

```bash
OPENTOPOGRAPHY_API_KEY=your-key cargo run --example minimal_globe -- --max-lod 7 --stream-height
```

Current height limits:
- `TERRAIN_STREAM_HEIGHT=1` also enables online imagery
- the first height provider uses OpenTopography `AW3D30_E`
- this is an ellipsoidal ALOS World 3D source, which keeps the height semantics closer to the renderer's WGS84 model
- unsupported regions fall back to the bundled starter Earth height

If you want an automated fly-in instead of manual camera movement, use the dedicated warmup demo:

```bash
source ./.env.opentopography.local
cargo run --example streaming_warmup_globe
```

Useful warmup overrides:
- `STREAM_WARMUP_EXIT_AFTER_SECONDS=45` for repeatable warmup runs
- `STREAM_WARMUP_TARGET_LAT` and `STREAM_WARMUP_TARGET_LON` to retarget the scripted descent
- `TERRAIN_STREAMING_MAX_LOD=11` if you want a deeper refinement ceiling than the default
- `TERRAIN_STREAM_HEIGHT=1` if you also want OpenTopography height during the warmup run
- `TERRAIN_STREAM_IMAGERY_PRESET=gibs_modis` if you want the original NASA GIBS imagery instead of the sharper default warmup preset

## 2. Preprocess a Dataset Without Downloading Anything

The repo also includes tiny tutorial rasters in `sample_data/`.
Once GDAL is installed, you can build a tutorial terrain locally:

```bash
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
cargo run --example minimal_globe -- terrains/tutorial_earth
```

This is the fastest path to validate that preprocessing works on your machine.

## 3. Recommended Cross-Platform GDAL Setup

Recommended path: install [Miniforge](https://github.com/conda-forge/miniforge), then create a small environment with GDAL from conda-forge.

Why this is the default recommendation:
- same package source across Windows, Linux, and macOS
- no project-specific shell script required
- predictable rollback path if you need to rebuild the environment
- the preprocess crate uses prebuilt GDAL bindings for supported GDAL versions, so you do not need a separate `libclang` setup

Official references:
- [GDAL download and install documentation](https://gdal.org/en/stable/download.html)
- [Miniforge project](https://github.com/conda-forge/miniforge)
- [georust/gdal build configuration](https://github.com/georust/gdal)

### macOS and Linux

```bash
conda create -n terrain-preprocess -c conda-forge gdal pkg-config
conda activate terrain-preprocess
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
cargo run --example minimal_globe -- terrains/tutorial_earth
```

Notes:
- `pkg-config` helps the Rust GDAL bindings locate the active GDAL installation cleanly
- if you prefer native package managers, they can work too, but Miniforge is the least fragmented path to document and support

### Windows PowerShell

```powershell
conda create -n terrain-preprocess -c conda-forge gdal
conda activate terrain-preprocess
$env:GDAL_HOME = "$env:CONDA_PREFIX\Library"
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
cargo run --example minimal_globe -- terrains/tutorial_earth
```

### Windows Command Prompt

```bat
conda create -n terrain-preprocess -c conda-forge gdal
conda activate terrain-preprocess
set GDAL_HOME=%CONDA_PREFIX%\Library
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
cargo run --example minimal_globe -- terrains/tutorial_earth
```

Why `GDAL_HOME` is documented explicitly on Windows:
- the Rust GDAL bindings look for headers and libraries relative to `GDAL_HOME`
- Miniforge places those files under `%CONDA_PREFIX%\Library`
- making that path explicit removes a common Windows build failure

## 4. Build Your Own Terrain

The CLI is intentionally generic, but there are two common attachment types:
- `height`: elevation data, usually exported as `r32f`
- `albedo`: color imagery, usually exported as `rg8u`

Height example:

```bash
cargo run -p bevy_terrain_preprocess -- \
  /path/to/dem.tif \
  assets/terrains/my_terrain \
  --overwrite \
  --lod-count 3 \
  --attachment-label height \
  --format r32f \
  --ts 128 \
  --bs 4 \
  --m 4
```

Albedo example:

```bash
cargo run -p bevy_terrain_preprocess -- \
  /path/to/imagery.tif \
  assets/terrains/my_terrain \
  --overwrite \
  --lod-count 3 \
  --attachment-label albedo \
  --format rg8u \
  --ts 128 \
  --bs 4 \
  --m 4
```

Then render it with the same minimal example:

```bash
cargo run --example minimal_globe -- terrains/my_terrain
```

## 5. Full Earth Rebuilds

If you already have local full-resolution sources, use [preprocess/examples/preprocess_earth.rs](../preprocess/examples/preprocess_earth.rs).
That example expects:
- `source_data/gebco_earth.tif`
- `source_data/true_marble.tif`

Those larger source files are intentionally not committed.

## 6. Common Footguns

- If rendering works but preprocessing does not, the missing dependency is almost always GDAL, not Bevy.
- If Windows preprocessing fails during the Rust build, check `GDAL_HOME` first.
- If you only want a visible globe, do not start with the preprocess pipeline.
- `cargo run --example minimal_globe` uses the bundled Earth by default. Pass a different terrain root as the first argument when you want to inspect another dataset.
- `TERRAIN_STREAMING_CACHE_ROOT` must be asset-relative. Use `streaming_cache`, not an absolute filesystem path.
- `TERRAIN_STREAM_ONLINE=1` is opt-in. Without it, the renderer never makes network requests.
- `TERRAIN_STREAMING_MAX_LOD` controls how far online refinement can go beyond the bundled starter dataset. The examples default to `10` when streaming is enabled.
- `TERRAIN_STREAM_HEIGHT=1` requires `OPENTOPOGRAPHY_API_KEY` and currently targets `AW3D30_E`.
- The preprocess CLI currently forces `GDAL_NUM_THREADS=1`. That is intentional until the custom transformer is safely cloneable across GDAL worker threads.
