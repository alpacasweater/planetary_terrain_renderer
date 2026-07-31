# EGM96 geoid grid

`WW15MGH.DAC` is the EGM96 15-arc-minute geoid-undulation grid (721 × 1440
big-endian `i16` centimetres), published by the U.S. National Geospatial-
Intelligence Agency (NGA) as public-domain reference data.

It is embedded at build time (`include_bytes!`) and parsed by
`small_world::egm96::EGM96::from_bytes`, so the renderer can convert streamed
OpenTopography DEM heights from EGM96-orthometric (≈MSL) to WGS84-ellipsoidal
(HAE) inside worker threads with no runtime file path. `small_world` is the
single source of truth for the geoid; see `docs/reference_frames.md`.

Validated against NGA reference undulations (bilinear): (0°, 0°) → +17.16 m;
Everest → −28.74 m; Indian Ocean low → ≈−107 m — all sub-metre.
