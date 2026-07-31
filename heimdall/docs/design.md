# heimdall — design

A ground-up design for a georeferenced asset-visualization globe: stream real terrain + satellite
imagery, correctly georeferenced, so real-world autonomous assets (positions, tracks, point clouds)
render in their true place with correct vertical references (HAE / MSL / AGL / ECEF), in real time.
Free data only. Single user for now; native, offline-capable, embeddable Rust.

heimdall is **not** a fork of `planetary_terrain_renderer`. It reuses that project's hard-won
*ideas* (`small_world` as frame authority, the vertical-datum contract, the streaming-robustness
lessons) on a much smaller, standards-based codebase. Crucially it **drops the cube-sphere SRS and
the Taylor-series precision trick** — those served the thesis's orbit-to-ground rendering novelty,
not our goal, and they were the bulk of the old code's complexity.

---

## 1. Goals and non-goals

**Goals**
- A WGS84 globe with streamed terrain geometry and satellite/aerial imagery draped on it.
- Survey-correct placement of assets given LLA (HAE or MSL), ECEF, or local ENU/NED, plus AGL.
- Real-time overlays: point markers, tracks (time-dynamic polylines), and point clouds.
- Explore a region online, then use it fully offline (tile caching + region prefetch).
- 100% free data, openly licensed **for caching**.

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
5. **Explicit vertical datums at every interface** (normative contract:
   [`reference_frames.md`](reference_frames.md)).

---

## 3. Architecture (five layers)

```
                +-----------------------------+
   feeds  --->  |  ingest    (positions, tracks, point clouds, time)                          |
                +--------------+--------------+
                               v
                +-----------------------------+
                |  assets    (ECS entities: geo-pose + datum -> render-space)                 |
                +--------------+--------------+
                               v
   small_world  |  frame     (WGS84/ECEF <-> LLA HAE/MSL <-> ENU/NED, geoid, floating origin) |
                +--------------+--------------+
                               ^
                +-----------------------------+
                |  render    (WebMercator quadtree LOD, terrain mesh, imagery drape)          |
                +--------------+--------------+
                               ^
                +-----------------------------+
                |  tile-data (fetch + decode + cache: Terrain-RGB, imagery XYZ/PMTiles)       |
                +-----------------------------+
```

### 3.1 frame (geodesy authority)
- Wraps `small_world`: `Lla{HAE|MSL} <-> Ecef <-> Ned/Enu`, geoid undulation (EGM96/EGM2008),
  AGL via a terrain-height query (see render).
- Owns the **ECEF → render-space** transform (`big_space` floating-origin rebase). One seam.
- The axis convention between geodetic ECEF and the engine's coordinate handedness is defined
  here, once.

### 3.2 tile-data (fetch + decode + cache)
- **TileSource** trait: `get(z, x, y) -> Bytes`, resolved cache-first (see §6).
- Decoders: Terrain-RGB PNG → `f32` heightfield; imagery PNG/JPEG → texture.
- **Datum conversion at ingest:** terrain heights are orthometric (Copernicus DEM = EGM2008); add
  geoid undulation to get ellipsoidal *before* they reach the renderer. Same contract and code
  shape as the predecessor's OpenTopography datum fix.
- Cache: SQLite (MBTiles schema) for the growing runtime cache; PMTiles for read-only region
  bundles; `prefetch(bbox, zmin..=zmax, layers)` for offline preparation.

### 3.3 render (globe + LOD)
- A screen-space-error-driven quadtree over WebMercator tiles (standard geospatial LOD).
- Per visible tile: build a grid mesh from the tile's heightfield, place each vertex on the WGS84
  ellipsoid at its (lat, lon, ellipsoidal height), texture with the imagery tile. Skirts hide
  seams between neighboring LODs.
- Precision via floating origin: vertices emitted relative to the current origin cell in f32;
  absolute ECEF never reaches the GPU.
- Keep decoded height tiles CPU-resident (LRU) so the frame layer can answer AGL queries
  (random-access terrain height) — the one genuinely required "custom" capability.

### 3.4 assets (overlays)
- ECS: an entity carries `GeoPose { position: LLA|ECEF|NED, datum, orientation }`; a system
  converts it to render-space each frame via the frame layer.
- Renderables: billboards/markers (instanced), tracks (line strips, optionally tubes), point
  clouds (GPU-instanced points; chunked + LOD for large clouds).
- Time-dynamic: a `Timeline` resource; positions/tracks interpolate to the current sim time.

### 3.5 ingest
- Pluggable feeds → assets. MVP: a file/replay source (CSV/GeoJSON) and a simple socket
  (UDP/WebSocket) for live positions. Point clouds via LAS/LAZ (`las`/`laz` crates).

---

## 4. Free data sources (with the licensing catch spelled out)

### Terrain — **Mapterhorn** (primary)
- Free global **Terrain-RGB** as **PMTiles** + COG; **Copernicus DEM 30 m** worldwide
  (swissALTI3D in CH). BSD-3 code, open-data attribution, hosted free on source.coop.
- **Datum:** Copernicus DEM is **EGM2008 orthometric** → convert to ellipsoidal at ingest.
- **Verify before coding the decoder:** Mapterhorn's exact Terrain-RGB formula (Mapbox-style
  `-10000 + 0.1·v` vs terrarium `(R·256+G+B/256) − 32768`) and whether its PMTiles are Mercator-
  or geographic-tiled.
- Alternatives: AWS Terrain Tiles (terrarium, open data); raw Copernicus DEM for regional insets.

### Imagery — **EOX Sentinel-2 cloudless** (primary) + **NASA GIBS** (floor)
- **Sentinel-2 cloudless** (EOX): global ~10 m, WMTS/XYZ in EPSG:3857 & :4326. **Caching/offline
  bundling is explicitly permitted.** License: **2016 layer = CC BY 4.0** (commercial OK);
  **2018–2024 = CC BY-NC-SA 4.0** (non-commercial). Fine for a single non-commercial user; revisit
  before any commercial use.
- **NASA GIBS**: unrestricted/public, but coarse (~250 m MODIS/VIIRS). Always-legal base layer and
  daily time-dynamic imagery.
- **US high-res:** USGS **NAIP** ~0.6–1 m, public domain, US-only.
- **Landmine — excluded by design:** Esri / Google / Bing / Mapbox "free" tiles prohibit
  persistent caching / offline storage in their ToS, which breaks the explore-then-offline
  workflow. There is no truly-free *global high-res* source; ~10 m (Sentinel-2) is the free
  ceiling.

---

## 5. Coordinate & datum design (the correctness core)

- **World frame:** ECEF (WGS84), rendered through a `big_space` floating origin. Assets never
  touch Mercator.
- **Tiling frame:** WebMercator (EPSG:3857), `z/x/y`. Closed-form forward/inverse Mercator only;
  used solely to know which tile covers which lat/lon and to place tile geometry.
- **Vertical datums** (enforced per interface — see [`reference_frames.md`](reference_frames.md)):
  - Asset ingest: accept HAE or MSL explicitly; convert to HAE at the boundary via `small_world`.
  - Terrain tiles: stored/rendered **ellipsoidal** (converted from EGM2008 orthometric at ingest).
  - AGL: renderer terrain-height query at the asset footprint, minus placement.
- **One geoid model** in the system (`small_world::egm96`; EGM96 and/or EGM2008 grid embedded).

Why this is correct: imagery/terrain are *textures and heightfields* draped on the true ellipsoid
at their real geodetic positions; asset math is pure WGS84/ECEF. WebMercator's spherical
distortion never contaminates positioning because it is only a data-indexing scheme, not a
coordinate frame.

---

## 6. Caching & offline

- **Runtime cache:** SQLite (MBTiles-compatible schema
  `tiles(zoom_level, tile_column, tile_row, tile_data)`) filled on demand while online.
- **Region bundles:** PMTiles (immutable, range-request friendly). Mapterhorn ships terrain as
  PMTiles natively; imagery bundles can be generated from cached tiles.
- **Prefetch:** `prefetch(bbox, zmin..=zmax, [terrain, imagery])` enumerates `z/x/y`, fetches, and
  stores — this *is* the "explore a region in advance" feature.
- **Resolution order per tile:** runtime cache → region PMTiles → remote HTTP (then cache the
  result). Fully offline once a region is prefetched.
- Cache entries are versioned by datum semantics (lesson from the predecessor: bump the cache
  namespace when the meaning of stored heights changes, never mix versions).

---

## 7. Tech / crate choices

| Concern | Choice |
| --- | --- |
| Engine / ECS / render | `bevy` (from M0) |
| Floating origin | `big_space` |
| Geodesy / datums / geoid | `small_world` (ours) |
| PMTiles read | `pmtiles` |
| Runtime tile cache | `rusqlite` (MBTiles schema) |
| HTTP fetch | `reqwest` or `ureq`, behind a `streaming` feature |
| Image decode | `image` (png/jpeg) |
| Point clouds | `las` / `laz`; custom chunked LOD later |
| Tile math | closed-form Mercator, zero deps (in-repo: `src/tiles.rs`) |

All free/open. The network stack is feature-gated so a pure-offline build drops HTTP entirely.

---

## 8. Rendering notes

- **LOD:** per-tile screen-space error (geometric error × pixels-per-metre vs threshold) drives
  quadtree split/merge. Cap concurrent tile builds.
- **Cracks:** vertical skirts per tile to start; edge snapping later if needed.
- **Terrain mesh:** fixed N×N grid per tile initially; RTIN/Martini adaptive meshing if triangle
  counts hurt.
- **Imagery:** one texture per tile, explicit gradients at seams. Height and imagery LODs may
  differ; sample the best available of each (ancestor fallback — never block, never hole).

---

## 9. Milestones

Summarized here; acceptance criteria live in [`roadmap.md`](roadmap.md).

- **M0 — Frame proof:** ellipsoid + camera + `big_space` + `small_world`; a marker at a known LLA
  sits exactly right. No tiles.
- **M1 — Imagery globe:** WebMercator quadtree LOD + GIBS imagery draped on the ellipsoid.
- **M2 — Terrain:** Mapterhorn Terrain-RGB, geoid-corrected; crack-free LOD; Sentinel-2 imagery.
- **M3 — Offline:** SQLite cache + region prefetch; renders with the network pulled.
- **M4 — Assets:** GeoPose entities, markers, tracks, AGL query, one live feed.
- **M5 — Point clouds:** LAS/LAZ + GPU points; chunked LOD.

M0–M2 is the risky/novel part; M3–M5 is mostly well-trodden integration.

---

## 10. Risks & open questions

- **Rust geospatial maturity.** No Cesium-grade terrain crate exists; we own the tile-LOD layer.
  Bounded, but it is the main build cost.
- **Terrain LOD crack handling** and triangle budgets at high zoom — standard but fiddly.
- **Imagery resolution.** 10 m global is the free ceiling; NAIP covers US detail. Confirm this is
  enough fidelity for asset visualization.
- **License drift.** Sentinel-2 cloudless NC terms if heimdall ever goes commercial or multi-user.
- **Point-cloud scale.** In-memory is fine for MVP; millions of points need an out-of-core octree.
- **Mapterhorn encoding + tiling scheme** — verify from their docs before writing the decoder.
- **small_world dependency pinning.** heimdall pins a git rev; keep the pinned rev reachable
  (merge the `from_bytes` work to small_world main and re-pin).

---

## 11. Inheritance from `planetary_terrain_renderer`

Carried over (as ideas and, where useful, code shape):
- `small_world` as the single geodesy source of truth; `docs/reference_frames.md` near-verbatim.
- Orthometric→ellipsoidal conversion at tile ingest (Copernicus DEM = EGM2008; same bug class as
  the OpenTopography fix, already solved once).
- Datum-versioned caches, atomic tile writes (temp+rename), serialized manifest updates.
- Streaming-robustness lessons as *design inputs*: failure memo + exponential backoff,
  event-driven tile promotion over per-frame stat polling, coarse-first fetch order, ancestor
  fallback (missing data degrades to coarser, never blocks, never holes).

Deliberately left behind: cube-sphere SRS, Taylor-series precision reconstruction, and the
bespoke split/downsample/stitch preprocessing pipeline.
