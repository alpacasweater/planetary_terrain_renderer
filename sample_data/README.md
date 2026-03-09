# Sample Data

This directory contains tiny committed rasters for the preprocess tutorial path.

Files:
- `gebco_earth_mini.tif`: starter height raster
- `true_marble_mini.tif`: starter color raster

Use them with:

```bash
cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth
cargo run --example minimal_globe -- terrains/tutorial_earth
```

These files exist to keep the first preprocess run reproducible and zero-download.
They are not intended to represent the full validated Earth dataset.
