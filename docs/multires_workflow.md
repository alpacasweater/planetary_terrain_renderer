# Multi-Resolution Workflow

This project can render a global base terrain and multiple local high-resolution
overlays at the same time.

Use this pattern to avoid rebuilding the full Earth dataset when you update
regional data.

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
  --lod-count 4 \
  --attachment-label height \
  --ts 512 \
  --bs 4 \
  --m 4 \
  --format r32f
```

Notes:
- Keep `--data-type float32` with `--format r32f` to avoid GPU upload size mismatches.
- Keep albedo in `assets/terrains/earth/albedo` as-is.

## 2. Build/Update Regional Overlay Only

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
  --lod-count 6 \
  --attachment-label height \
  --ts 512 \
  --bs 4 \
  --m 4 \
  --format r32f
```

`--create-mask` ensures only covered pixels override the base terrain.

## 3. Example Regional Builds

Swiss:

```bash
./target/debug/bevy_terrain_preprocess \
  source_data/swiss.tif \
  assets/terrains/swiss_highres \
  --temp-path /tmp/swiss_highres_tmp \
  --overwrite --no-data source --data-type float32 --fill-radius 0 \
  --create-mask --lod-count 6 --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
```

LOS:

```bash
./target/debug/bevy_terrain_preprocess \
  source_data/LOS.tiff \
  assets/terrains/los_highres \
  --temp-path /tmp/los_highres_tmp \
  --overwrite --no-data source --data-type float32 --fill-radius 0 \
  --create-mask --lod-count 6 --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
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
    --create-mask --lod-count 6 --attachment-label height --ts 512 --bs 4 --m 4 --format r32f
done
```

## 4. Run Multi-Resolution Example

```bash
cargo run --example spherical_multires
```

The example always loads:
- `terrains/earth/config.tc.ron` as base.

By default it loads only:
- `terrains/swiss_highres/config.tc.ron`

You can choose overlays explicitly with `MULTIRES_OVERLAYS`:

```bash
# no overlays (base earth only)
MULTIRES_OVERLAYS=none cargo run --example spherical_multires

# swiss + los
MULTIRES_OVERLAYS=swiss,los cargo run --example spherical_multires

# saxony partial overlay (from source_data/saxony_dgm1/extracted)
MULTIRES_OVERLAYS=saxony cargo run --example spherical_multires

# selected SRTM tiles
MULTIRES_OVERLAYS=srtm_n27e086,srtm_n39w077 cargo run --example spherical_multires
```

## 5. Visual Validation Tips

- Press `L` for terrain data LOD view.
- Press `Y` for geometry LOD view.
- Press `Q` for tile tree view.
- Orbit from far to near over an overlay region and verify only local patches
  gain higher detail.
