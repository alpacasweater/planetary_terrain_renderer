# Physical Truth Mapping Audit (2026-03-07)

## Root Cause
The renderer had two coupled WGS84 mistakes:

1. Geodetic latitude/longitude was being treated like a spherical angle on the renderer unit sphere.
2. CPU-side ellipsoid height offsets were using the radial direction instead of the ellipsoid normal.

That combination produced kilometer-scale local-position error for direct
`Coordinate::from_lat_lon_degrees(...).local_position(TerrainShape::WGS84, hae)` placement.

## Affected Path

### Preprocess
- `preprocess/src/transformers.rs`
- `CubeTransformer`
- GDAL reprojection correctly produced WGS84 geodetic lon/lat
- renderer code then converted that lon/lat with unit-sphere spherical formulas

### Runtime / CPU helpers
- `src/math/coordinate.rs`
- `src/math/geodesy.rs`
- `src/math/terrain_shape.rs`

Direct `lat/lon -> unit chart -> local position` was therefore not equivalent to true WGS84 ECEF.

## Fix Implemented

### Geodetic lat/lon to renderer unit chart
Now computed as:

1. geodetic `lat/lon` -> WGS84 ECEF surface point
2. ECEF -> renderer local axes
3. divide by renderer ellipsoid scale `(a, b, a)`
4. normalize to the renderer unit chart

### Renderer unit chart to geodetic lat/lon
Now computed as:

1. renderer unit chart -> renderer local ellipsoid surface point
2. renderer local axes -> ECEF
3. ECEF -> geodetic `lat/lon`

### CPU ellipsoid height offset
For `TerrainShape::Spheroid`, the CPU path now uses the ellipsoid normal:

`normalize(unit_position / scale)`

instead of the radial direction:

`normalize(scale * unit_position)`

## Verified Result

Validated by:

```bash
cargo test math::geodesy::tests:: -- --nocapture
```

Key passing tests:
- `unit_from_lat_lon_matches_small_world_wgs84_surface`
- `renderer_wgs84_local_mapping_matches_small_world`

These now match `small_world` to floating-point tolerance.

## Remaining Consequence
Existing terrain assets that were preprocessed with the old semantics are stale.

That means:
- direct renderer-native `lat/lon+HAE` placement is now fixed in code
- but previously generated terrain tiles still encode the old preprocess mapping

So ground/path residual metrics must be re-run after rebuilding affected terrain assets.
