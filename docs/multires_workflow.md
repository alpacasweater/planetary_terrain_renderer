# Multi-Resolution Workflow

This project renders a global base terrain and multiple local high-resolution overlays at the same time.

Use this pattern to avoid rebuilding the full Earth dataset when you update regional data.

Important:
- Do not blindly reuse an old `--lod-count` for regional overlays. The correct target depends on the source raster resolution and geographic extent.
- The preprocess CLI now forces GDAL warp to single-threaded mode because the current custom transformer is not yet safely cloneable across GDAL worker threads.

## 1. Build Global Base Once

```bash
cd /path/to/planetary_terrain_renderer

./target/debug/bevy_terrain_preprocess \
  source_data/gebco_earth_small.tif \
  assets/terrains/earth \
  --temp-path /tmp/terrain_height_tmp \
  --overwrite \
  --no-data source \
  --data-type float32 \
  --fill-radius 0 \
  --lod-count 5 \
  --attachment-label height \
  --ts 512 \
  --bs 4 \
  --m 4 \
  --format r32f
```

Notes:
- Keep `--data-type float32` with `--format r32f` to avoid GPU upload size mismatches.
- For `source_data/gebco_earth_small.tif`, `--lod-count 5` is the current physical-truth target for the base Earth height asset.
- Keep albedo in `assets/terrains/earth/albedo` as-is.

## 2. Build or Update A Regional Overlay Only

Template:

```bash
./target/debug/bevy_terrain_preprocess \
  <regional_input.tif> \
  assets/terrains/<overlay_name> \
  --temp-path /tmp/<overlay_name>_tmp \
  --overwrite \
  --no-data source \
  --data-type float32 \
  --fill-radius 0 \
  --create-mask \
  --attachment-label height \
  --ts 512 \
  --bs 4 \
  --m 4 \
  --format r32f
```

`--create-mask` ensures only covered pixels override the base terrain.

Recommendation:
- Omit `--lod-count` unless you are intentionally trading truth for size.
- Let the preprocess heuristic choose the required face resolution for the source DEM.

## 3. Example Regional Builds

Swiss:

```bash
./target/debug/bevy_terrain_preprocess \
  source_data/swiss.tif \
  assets/terrains/swiss_highres \
  --temp-path /tmp/swiss_highres_tmp \
  --overwrite --no-data source --data-type float32 --fill-radius 0 \
  --create-mask --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
```

Measured result:
- the Swiss source DEM is `80 m` resolution
- the preprocess heuristic selects `lod_count = 9`
- that build is about `346M` locally and reduces source-raster parity to about `9.5-10.1 m RMS` and `17.7-22.0 m p95` across sampled mountain cases

LOS:

```bash
./target/debug/bevy_terrain_preprocess \
  source_data/LOS.tiff \
  assets/terrains/los_highres \
  --temp-path /tmp/los_highres_tmp \
  --overwrite --no-data source --data-type float32 --fill-radius 0 \
  --create-mask --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
```

SRTM tiles (one overlay per file):

```bash
for f in source_data/srtm_tif/*.tif; do
  name=$(basename "$f" .tif | tr "[:upper:]" "[:lower:]")
  ./target/debug/bevy_terrain_preprocess \
    "$f" \
    "assets/terrains/srtm_${name}" \
    --temp-path "/tmp/${name}_tmp" \
    --overwrite --no-data source --data-type float32 --fill-radius 0 \
    --create-mask --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
done
```

## 4. Run The Multi-Resolution Example

```bash
cargo run --example spherical_multires
```

The example always loads:
- `terrains/earth/config.tc.ron` as base

By default it loads only:
- `terrains/swiss_highres/config.tc.ron`

You can choose overlays explicitly with `MULTIRES_OVERLAYS`:

```bash
# no overlays (base earth only)
MULTIRES_OVERLAYS=none cargo run --example spherical_multires

# swiss + los
MULTIRES_OVERLAYS=swiss,los cargo run --example spherical_multires

# saxony partial overlay
MULTIRES_OVERLAYS=saxony cargo run --example spherical_multires

# selected SRTM tiles
MULTIRES_OVERLAYS=srtm_n27e086,srtm_n39w077 cargo run --example spherical_multires
```

Swiss drone demo:

```bash
MULTIRES_OVERLAYS=swiss \
MULTIRES_ENABLE_DRONE=1 \
MULTIRES_DRONE_AGL_M=250 \
MULTIRES_DRONE_ORBIT_RADIUS_M=1500 \
MULTIRES_DRONE_PERIOD_SECONDS=12 \
MULTIRES_DRONE_SIZE_M=260 \
cargo run --example spherical_multires
```

Notes:
- the drone orbit is precomputed from the Swiss source raster so the rendered path height is tied to the overlay DEM rather than an arbitrary offset
- the cyan breadcrumb trail shows the orbit samples; the orange sphere is the live drone position

## 5. Visual Validation Tips

- Press `L` for terrain data LOD view.
- Press `Y` for geometry LOD view.
- Press `Q` for tile tree view.
- Orbit from far to near over an overlay region and verify only local patches gain higher detail.
