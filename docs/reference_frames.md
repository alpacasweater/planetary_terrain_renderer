# Reference Frames & Vertical Datums

This renderer displays the reported state of real, georeferenced entities (terrain,
autonomous agents). Getting the vertical reference wrong is not cosmetic: **HAE, MSL, and
AGL differ by up to ~100 m of geoid undulation plus the full terrain relief**, so every
interface that carries an altitude must state its datum explicitly and convert at a single,
well-defined boundary.

`small_world` is the **single source of truth** for all reference-frame math. The renderer
never re-implements ellipsoid, geoid, or local-frame formulas; it calls small_world through
the thin facade in [`src/math/geodesy.rs`](../src/math/geodesy.rs).

## Frames

| Frame | Meaning | Positive dir | Provider |
| --- | --- | --- | --- |
| **LLA (HAE)** | latitude/longitude + height above the WGS84 **ellipsoid** | up | `small_world::wgs84::Lla` (`AltType::Wgs84`) |
| **MSL** | orthometric height above the **geoid** (mean sea level) | up | `small_world::egm96` (EGM96, `WW15MGH.DAC`) |
| **AGL** | height above the local **terrain surface** | up | the renderer's terrain query (small_world cannot answer it) |
| **ECEF** | Earth-Centred Earth-Fixed metres, WGS84 axes | — | `small_world::wgs84::Ecef` |
| **NED / ENU** | local tangent frame about an origin | N/E/D, E/N/U | `small_world::wgs84::{Ned, Enu}` |

`HAE = MSL + N`, where `N` is the geoid undulation (`geoid.offset_bilinear(lat, lon)`).
`N` ranges roughly −107 m … +85 m worldwide.

## The renderer's canonical frame: **HAE**

The renderer's ellipsoid chart is defined against the WGS84 ellipsoid, so the one frame that
flows through the render path is **HAE**:

- `Coordinate::local_position(shape, height)` interprets `height` as **HAE metres** above the
  WGS84 ellipsoid surface.
- `geodesy::{lla_hae_to_ecef, ecef_to_lla_hae, renderer_local_to_lla_hae, ned_to_ecef,
  ecef_to_ned}` all operate in HAE.

Everything else is converted to HAE **at the ingest boundary**, never deep in the render loop.

## Per-interface contract

### 1. Agent-state ingest → **HAE (enforced)**

The renderer positions agents in HAE. A caller holding agent altitude in another datum MUST
convert at the boundary before handing a position to the renderer, using small_world:

```rust
use small_world::altitude::{AltitudeConverter, GeoPoint, VerticalFrame};
// geoid: &EGM96, terrain: &dyn TerrainProvider
let hae_m = AltitudeConverter::new(geoid, terrain)
    .convert_height_m(GeoPoint::new(lat, lon)?, reported_alt_m, source_frame, VerticalFrame::Hae)?;
```

- If the agent feed reports **MSL**, convert MSL→HAE (needs the geoid only).
- If it reports **AGL**, convert AGL→HAE (needs terrain ground elevation **and** the geoid).
- If it already reports **HAE**, pass it through unchanged.

Rationale: HAE is the only datum the ellipsoid chart can consume without a per-frame geoid
lookup on the hot path, and it is unambiguous (independent of terrain data availability).

### 2. Terrain tile heights → **ellipsoidal (HAE), post-ingest**

Cached terrain tiles store **ellipsoidal** heights so the render path never needs a geoid
lookup. Upstream DEM sources are **not** ellipsoidal:

- OpenTopography SRTM/AW3D30/NASADEM heights are **EGM96-orthometric (≈MSL)**.

These are converted **orthometric → HAE at tile-write time** by adding the geoid undulation
`N(lat, lon)` (the geoid is smooth at tile scale, so evaluating `N` at the tile corners and
bilinearly interpolating is sufficient and effectively free). Because this changes the meaning
of cached tiles, the datum-ingest fix bumps `geodetic_mapping_version`; a cache written by an
older renderer is rejected rather than silently mixed.

### 3. AGL → **renderer terrain query**

AGL is answered by the renderer, not by a geodesy library: sample the loaded terrain height at
the agent's footprint (the thesis's random-access terrain-data requirement exists for exactly
this) and subtract to obtain the ground-relative height. small_world's `TerrainProvider` covers
offline DEM datasets, but the *rendered* globe's AGL is the renderer's own terrain query.

## Invariant

There is exactly one geoid model in the system (`small_world::egm96`, EGM96) and one place the
geodetic-ECEF ↔ renderer-local axis swap `(x, y, z) → (−x, z, y)` lives
([`src/math/geodesy.rs`](../src/math/geodesy.rs)). Any new interface that carries an altitude
must name its datum and convert to HAE at its boundary.
