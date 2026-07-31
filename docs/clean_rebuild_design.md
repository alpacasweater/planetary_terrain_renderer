# Clean rebuild: georeferenced asset-visualization globe

Working name: **asset-globe** (placeholder).

A ground-up design for the tool we actually want: stream real terrain + satellite imagery,
correctly georeferenced, so real-world autonomous assets (positions, tracks, point clouds) render
in their true place with correct vertical references (HAE / MSL / AGL / ECEF), in real time. Free
data only. Single user for now; native, offline-capable, embeddable Rust.

This is **not** a fork of `planetary_terrain_renderer`. It reuses that project's hard-won *ideas*
(`small_world` as frame authority, the vertical-datum contract, the streaming-robustness lessons)
on a much smaller, standards-based codebase. Crucially it **drops the cube-sphere SRS and the
Taylor-series precision trick** — those served the thesis's orbit-to-ground rendering novelty, not
our goal, and they are the bulk of the old code's complexity.

---

## 1. Goals and non-goals

**Goals**
- A WGS84 globe with streamed terrain geometry and satellite/aerial imagery draped on it.
- Survey-correct placement of assets given LLA (HAE or MSL), ECEF, or local ENU/NED, plus AGL.
- Real-time overlays: point markers, tracks (time-dynamic polylines), and point clouds.
- Explore a region online, then use it fully offline (tile caching + region prefetch).
- 100% free data and permissively/openly licensed for caching.

**Non-goals (for now)**
- Seamless orbit-to-surface rendering of the *whole* planet at once (thesis territory).
- Photoreal 3D buildings / global 3D Tiles meshes.
- Multi-user / server backend.
- Commercial redistribution (affects imagery-license choice; see §4).

---

## 2. Guiding principles

1. **Georeferencing correctness is the product.** Every asset position flows through one geodesy
   authority (`small_world`) into one world frame. No ad-hoc math anywhere else.
2. **Use the world's standards; don't invent tiling.** Terrain and imagery are published in the
   WebMercator (slippy-map) quadtree. Tile in that scheme so a fetched tile is used as-is — this
   is what makes the old reprojection/preprocessing subsystem disappear.
3. **Tiling projection ≠ positioning frame.** Data is tiled in WebMercator; geometry and assets
   live on the true WGS84 ellipsoid in ECEF. The two only meet when a tile's Mercator extent is
   inverse-projected to lat/lon and its vertices/texels are placed on the ellipsoid.
4. **Floating origin for precision.** Rebase the world origin near the camera (`big_space`); render
   everything in f32 relative to it. Simple, standard, sufficient when the camera is near assets.
5. **Explicit vertical datums at every interface** (carried over from `docs/reference_frames.md`).

---

## 3. Architecture (five layers)

```
                +-----------------------------+
   feeds  --->  |  ingest    (positions, tracks, point clouds, time) |
                +--------------+--------------+
                               v
                +-----------------------------+
                |  assets    (ECS entities: geo-pose + datum -> render-space) |
                +--------------+--------------+
                               v
   small_world  |  frame     (WGS84/ECEF <-> LLA HAE/MSL <-> ENU/NED, geoid, floating origin) |
                +--------------+--------------+
                               ^
                +-----------------------------+
                |  render    (WebMercator quadtree LOD, terrain mesh, imagery drape, precision) |
                +--------------+--------------+
                               ^
                +-----------------------------+
                |  tile-data (fetch + decode + cache: terrain-RGB, imagery XYZ/PMTiles) |
                +-----------------------------+
```

### 3.1 frame (geodesy authority)
- Wraps `small_world`: `Lla{HAE|MSL} <-> Ecef <-> Ned/Enu`, geoid undulation (EGM96/EGM2008),
  AGL via a terrain-height query (see render).
- Owns the **ECEF -> render-space** transform: `render_pos = big_space_rebase(ecef)`. One seam.
- Provides the axis convention once (the old repo's `(x,y,z) -> (-x,z,y)` lives here or is dropped
  in favor of using ECEF axes directly with a fixed root rotation).

### 3.2 tile-data (fetch + decode + cache)
- **TileSource** trait: `get(z,x,y) -> Bytes`, backed by (a) a local read-write cache first, then
  (b) a remote HTTP endpoint, then (c) a bundled PMTiles archive.
- Decoders: Terrain-RGB PNG -> `f32` heightfield; imagery PNG/JPEG -> texture.
- **Datum conversion at ingest:** terrain heights are orthometric (Copernicus DEM = EGM2008); add
  geoid undulation to get ellipsoidal before they reach the renderer. Reuse the exact contract and
  code shape from the old repo's `opentopography` datum fix.
- Cache: SQLite (MBTiles schema `tiles(z,y,x,blob)`) for the growing runtime cache; PMTiles for
  read-only region bundles. A `prefetch(bbox, zmin..=zmax, layers)` fills the cache for offline use.

### 3.3 render (globe + LOD)
- A screen-space-error-driven quadtree over WebMercator tiles (standard geospatial LOD).
- Per visible tile: build a grid mesh from the tile's heightfield, place each vertex on the WGS84
  ellipsoid at its (lat, lon, ellipsoidal height), texture it with the imagery tile. Add skirts to
  hide seams between neighboring LODs.
- Precision via floating origin: vertices emitted relative to the current origin cell in f32.
- Keep decoded height tiles CPU-resident (LRU) so the frame layer can answer AGL queries
  (random-access terrain height) — the one genuinely required "custom" capability.

### 3.4 assets (overlays)
- ECS: an entity carries a `GeoPose { position: LLA|ECEF|NED, datum, orientation }` component; a
  system converts it to render-space each frame via the frame layer.
- Renderables: billboards/markers (instanced), tracks (line strips, optionally tube meshes),
  point clouds (GPU-instanced points; chunked + LOD for large clouds).
- Time-dynamic: a `Timeline` resource; positions/tracks interpolate to the current sim time.

### 3.5 ingest
- Pluggable feeds -> `assets`. MVP: a file/replay source (CSV/GeoJSON/CZML-like) and a simple
  socket (UDP/WebSocket/ZeroMQ) for live positions. Point clouds via LAS/LAZ (`las`/`laz` crates).

---

## 4. Free data sources (with the licensing catch spelled out)

### Terrain — **Mapterhorn** (primary)
- Free global **Terrain-RGB** as **PMTiles** + COG; **Copernicus DEM 30 m** worldwide
  (swissALTI3D in CH). BSD-3 code, open-data attribution, hosted free on source.coop, MapLibre-ready.
- **Datum:** Copernicus DEM is **EGM2008 orthometric** -> convert to ellipsoidal at ingest.
- **Encoding:** verify Mapterhorn's exact Terrain-RGB formula against their spec before decoding
  (Mapbox-style base `-10000`, factor `0.1` vs terrarium `(R*256+G+B/256)-32768`).
- Alternatives: AWS Terrain Tiles (terrarium, open data); Copernicus DEM / OpenTopography raw for
  higher-res regional insets.

### Imagery — **EOX Sentinel-2 cloudless** (primary) + **NASA GIBS** (floor)
- **Sentinel-2 cloudless** (EOX): global ~10 m, WMTS/XYZ in EPSG:3857 & :4326. **Caching/offline
  bundling is explicitly permitted.** License: **2016 = CC BY 4.0** (commercial OK); **2018–2024 =
  CC BY-NC-SA 4.0** (non-commercial). Fine for a single non-commercial user; revisit if it ever
  ships commercially (use the 2016 layer or another source then).
- **NASA GIBS**: unrestricted/public, but coarse (~250 m MODIS/VIIRS). Always-legal base + daily
  time-dynamic imagery.
- **US high-res:** USGS **NAIP** ~0.6–1 m, public domain, US-only.
- **Landmine — do not use:** Esri / Google / Bing / Mapbox "free" tiles **prohibit persistent
  caching / offline storage** in their ToS; that breaks the explore-then-offline workflow. There is
  no truly-free *global high-res* source — 10 m (Sentinel-2) is the realistic free ceiling.

---

## 5. Coordinate & datum design (the correctness core)

- **World frame:** ECEF (WGS84), rendered through a `big_space` floating origin. Assets never touch
  Mercator.
- **Tiling frame:** WebMercator (EPSG:3857), `z/x/y`. Closed-form forward/inverse Mercator only;
  used solely to know which tile covers which lat/lon and to place tile geometry.
- **Vertical datums** (enforced per interface, per `docs/reference_frames.md`):
  - Asset ingest: accept HAE or MSL explicitly; convert to HAE at the boundary via `small_world`.
  - Terrain tiles: stored/rendered **ellipsoidal** (converted from EGM2008 orthometric at ingest).
  - AGL: renderer terrain-height query at the asset footprint, minus placement.
- **One geoid model** in the system (`small_world::egm96`, EGM96 or EGM2008 grid embedded).

Why this is correct: imagery/terrain are *textures and heightfields* draped on the true ellipsoid at
their real geodetic positions; asset math is pure WGS84/ECEF. WebMercator's spherical distortion
never contaminates positioning because it is only a data-indexing scheme, not a coordinate frame.

---

## 6. Caching & offline

- **Runtime cache:** SQLite (MBTiles-compatible) `tiles(zoom_level, tile_column, tile_row, tile_data)`
  filled on demand as you pan/zoom.
- **Region bundles:** PMTiles (immutable, range-request friendly). Mapterhorn ships terrain as
  PMTiles; imagery bundles can be generated from fetched tiles.
- **Prefetch:** `prefetch(bbox, zmin..=zmax, [terrain, imagery])` enumerates `z/x/y`, fetches, and
  stores — this *is* the "explore a region in advance" feature.
- **Resolution order per tile:** runtime cache -> region PMTiles -> remote HTTP (then cache the
  result). Fully offline once a region is prefetched.

---

## 7. Tech / crate choices

| Concern | Choice |
| --- | --- |
| Engine / ECS / render | `bevy` |
| Floating origin | `big_space` |
| Geodesy / datums / geoid | `small_world` (already ours) |
| PMTiles read | `pmtiles` |
| Runtime tile cache | `rusqlite` (MBTiles schema) |
| HTTP fetch | `reqwest` (async) or `ureq`; behind a `streaming` feature |
| Image decode | `image` (png/jpeg) |
| Point clouds | `las` / `laz`; custom chunked-octree LOD later |
| Tile math | closed-form Mercator (no dep) |

All free/open. Feature-gate the network stack so a pure-offline build drops HTTP.

---

## 8. Rendering notes

- **LOD:** per-tile screen-space-error (geometric error * pixels-per-metre vs a threshold); split /
  merge the quadtree accordingly. Cap concurrent tile builds.
- **Cracks:** vertical skirts around each tile, or edge-vertex snapping to the coarser neighbor.
- **Terrain mesh:** fixed NxN grid per tile is fine to start; RTIN/Martini adaptive meshing later if
  triangle counts hurt.
- **Precision:** never upload absolute ECEF to the GPU; subtract the floating-origin cell first.
- **Imagery:** one texture per tile, sampled with explicit gradients at seams. Height and imagery
  LODs can differ; sample the best available of each (ancestor fallback), same principle as the fork.

---

## 9. Milestones (to "first light" and beyond)

- **M0 — Frame proof.** Ellipsoid + orbit camera + `big_space` + `small_world`. Drop a marker at a
  known LLA (e.g. a survey benchmark) and confirm it sits exactly right at multiple zooms. *No tiles
  yet.* This validates the whole correctness core cheaply.
- **M1 — Imagery globe.** WebMercator quadtree LOD + stream **GIBS** (unrestricted) draped on the
  ellipsoid. Pan/zoom a region.
- **M2 — Terrain.** Mapterhorn Terrain-RGB, geoid-corrected to ellipsoidal; crack-free LOD terrain;
  swap imagery to Sentinel-2 cloudless.
- **M3 — Offline.** SQLite cache + `prefetch(region)`; pull the network and confirm the region still
  renders.
- **M4 — Assets.** GeoPose entities, markers, tracks, AGL query, one live feed.
- **M5 — Point clouds.** LAS/LAZ load + GPU points; chunked LOD for large clouds.

M0–M2 is the risky/novel part and is a few focused weeks; M3–M5 is mostly well-trodden integration.

---

## 10. Risks & open questions

- **Rust geospatial maturity.** No Cesium-grade terrain/3D-tiles crate exists; we own the tile-LOD
  layer. Bounded, but it's the main build cost.
- **Terrain LOD crack handling** and triangle budgets at high zoom — standard but fiddly.
- **Imagery resolution.** 10 m global is the free ceiling; NAIP for US detail. Confirm that's enough
  for the asset-viz fidelity you need.
- **License drift.** Sentinel-2 cloudless NC terms if this ever goes commercial or multi-user.
- **Point-cloud scale.** In-memory is fine for MVP; millions of points need an out-of-core octree.
- **Mapterhorn encoding + tiling scheme** (Mercator vs geographic PMTiles) — verify from their docs
  before writing the decoder.

---

## 11. What carries over from `planetary_terrain_renderer`

- `small_world` as the single geodesy source of truth, and `docs/reference_frames.md` verbatim.
- The orthometric->ellipsoidal ingest conversion (Copernicus DEM = EGM2008, same class of bug).
- Cache-versioning (per-datum cache dir) and atomic tile writes.
- Streaming-robustness lessons as design inputs: failure memo + backoff, event-driven promotion
  over per-frame stat storms, ancestor fallback, atomic writes. On the clean codebase these become
  small, well-scoped modules instead of a defect list.

What we deliberately leave behind: cube-sphere SRS, the Taylor-series precision reconstruction, and
the bespoke split/downsample/stitch preprocessing pipeline.
