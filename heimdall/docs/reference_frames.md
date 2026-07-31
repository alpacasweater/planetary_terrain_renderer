# Reference frames & vertical datums (normative)

heimdall displays the reported state of real, georeferenced assets. Getting the vertical reference
wrong is not cosmetic: **HAE, MSL, and AGL differ by up to ~100 m of geoid undulation plus the full
terrain relief**, so every interface that carries an altitude must state its datum explicitly and
convert at a single, well-defined boundary.

[`small_world`](https://github.com/Swarm-Command/small_world) is the **single source of truth** for
all reference-frame math. heimdall never re-implements ellipsoid, geoid, or local-frame formulas;
it calls small_world through the thin facade in `src/frame.rs`.

## Frames

| Frame | Meaning | Positive dir | Provider |
| --- | --- | --- | --- |
| **LLA (HAE)** | latitude/longitude + height above the WGS84 **ellipsoid** | up | `small_world::wgs84::Lla` (`AltType::Wgs84`) |
| **MSL** | orthometric height above the **geoid** (mean sea level) | up | `small_world::egm96` (EGM96 / EGM2008 grids) |
| **AGL** | height above the local **terrain surface** | up | heimdall's terrain-height query (small_world cannot answer it) |
| **ECEF** | Earth-Centred Earth-Fixed metres, WGS84 axes | — | `small_world::wgs84::Ecef` |
| **NED / ENU** | local tangent frame about an origin | N/E/D, E/N/U | `small_world::wgs84::{Ned, Enu}` |

`HAE = MSL + N`, where `N` is the geoid undulation (`geoid.offset_bilinear(lat, lon)`).
`N` ranges roughly −107 m … +85 m worldwide.

## The canonical frame: **HAE on the WGS84 ellipsoid, positioned in ECEF**

The render world is the true WGS84 ellipsoid in ECEF (through a floating origin). The one vertical
frame that flows through the render path is **HAE**. Everything else converts to HAE **at its
ingest boundary**, never deep in the render loop.

**WebMercator is not a coordinate frame.** It is the tiling index for streamed data (`z/x/y`).
Tile extents are inverse-projected to lat/lon and placed on the ellipsoid; asset math never touches
Mercator.

## Per-interface contract

### 1. Asset-state ingest → **HAE (enforced)**

A caller holding an asset altitude in another datum MUST convert at the boundary:

```rust
use small_world::altitude::{AltitudeConverter, GeoPoint, VerticalFrame};
let hae_m = AltitudeConverter::new(geoid, terrain)
    .convert_height_m(GeoPoint::new(lat, lon)?, reported_alt_m, source_frame, VerticalFrame::Hae)?;
```

- **MSL** feed → MSL→HAE (geoid only).
- **AGL** feed → AGL→HAE (terrain ground elevation **and** geoid).
- **HAE** feed → pass through.
- **ECEF** feed → already unambiguous; convert directly.

Rationale: HAE is the only datum the ellipsoid can consume without a per-frame geoid lookup on the
hot path, and it is unambiguous (independent of terrain-data availability).

### 2. Terrain tile heights → **ellipsoidal (HAE), converted at ingest**

Cached terrain tiles store **ellipsoidal** heights so the render path never needs a geoid lookup.
Upstream DEMs are **not** ellipsoidal:

- Copernicus DEM (Mapterhorn's global source) is **EGM2008-orthometric**.
- SRTM/AW3D30-class sources are **EGM96-orthometric**.

Convert **orthometric → HAE at tile-decode/cache-write time** by adding `N(lat, lon)`. The geoid
is smooth at tile scale: undulation at tile corners + bilinear interpolation is sufficient and
effectively free. Because this changes the meaning of cached bytes, the cache namespace is
versioned by datum semantics; versions are never mixed.

### 3. AGL → **heimdall terrain query**

AGL is answered by the renderer, not by a geodesy library: sample the loaded terrain height at the
asset's footprint (decoded height tiles stay CPU-resident for random access) and subtract.
small_world's `TerrainProvider` covers offline DEM datasets, but the *rendered* globe's AGL is
heimdall's own terrain query.

## Invariants

1. Exactly **one geoid model** in the system (`small_world::egm96`), embedded at build time.
2. Exactly **one place** the geodetic-ECEF ↔ engine-axis convention lives (`src/frame.rs`).
3. Any new interface that carries an altitude must name its datum and convert to HAE at its
   boundary.

## Validation anchors

NGA EGM96 reference undulations (used to validate small_world's geoid, bilinear, all sub-metre):

| Location | Reference N |
| --- | --- |
| (0°N, 0°E) | **+17.16 m** |
| Everest (27.9881°N, 86.925°E) | **−28.74 m** |
| Indian Ocean low (4.75°N, 78.75°E) | **≈ −107 m** |
