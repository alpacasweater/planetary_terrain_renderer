# heimdall — roadmap

Milestones are ordered so the *correctness core* is proven before any pixels stream, and each
milestone has a concrete acceptance test. Estimates assume one engineer, part-time focus.

---

## M0 — Frame proof (no tiles)

**Build:** Bevy app + `big_space` floating origin + WGS84 ellipsoid mesh (untextured) + orbit
camera + `small_world`-backed `frame` layer. Place a marker entity from a hardcoded LLA.

**Accept when:**
- A marker placed at a known survey point (e.g. the Greenwich Observatory, or any benchmark with
  published LLA-HAE) sits visually on the ellipsoid surface at every zoom level, with no jitter
  when the camera is metres away (floating origin works).
- Round-trip test: marker LLA → ECEF → render-space → back to LLA within 1e-9° / 1e-4 m.
- `cargo test` covers frame conversions and the engine-axis convention.

This milestone is cheap and de-risks everything: if M0 is right, georeferencing is right forever.

## M1 — Imagery globe

**Build:** WebMercator quadtree with screen-space-error LOD; stream **NASA GIBS** (unrestricted)
XYZ tiles; drape each tile on the ellipsoid at its true geodetic extent.

**Accept when:**
- Pan/zoom from globe view to ~z12 over any region; tiles refine without holes (ancestor
  fallback) and without seams at tile borders.
- The M0 marker at a recognizable coastline/landmark sits on the correct imagery pixel.

## M2 — Terrain

**Build:** Mapterhorn Terrain-RGB decode (verify encoding + tiling scheme first) →
orthometric→HAE conversion at ingest (EGM2008 grid via `small_world`) → per-tile grid meshes with
skirts; switch imagery to EOX Sentinel-2 cloudless.

**Accept when:**
- Everest region renders with visibly correct relief; spot-check: terrain surface height under a
  benchmark matches published elevation within source accuracy (~±10 m for Copernicus 30 m).
- Datum regression test: synthetic Terrain-RGB tile in → cached heights out; ellipsoidal offset
  equals `N` at tile centre (the predecessor's datum-bug test, ported).
- No cracks between LOD levels in normal navigation.

## M3 — Offline

**Build:** SQLite (MBTiles-schema) runtime cache; cache-first tile resolution; PMTiles bundle
reader; `prefetch(bbox, zmin..=zmax, layers)` CLI/command.

**Accept when:**
- Prefetch a region at z ≤ N, disable networking entirely, restart: the region renders fully
  (terrain + imagery); outside the region degrades gracefully to whatever coarse levels exist.
- Cache writes are atomic (kill -9 during prefetch leaves a valid cache).

## M4 — Assets

**Build:** `GeoPose` component (position + explicit datum + orientation); ingest of a replay file
and one live socket feed; markers + tracks; AGL query against resident terrain; time scrubbing.

**Accept when:**
- An asset fed identical positions expressed as HAE, MSL, and (AGL + ground) renders at the same
  spot (datum conversions agree end-to-end).
- A simulated flight track drapes correctly over terrain; reported AGL matches
  (HAE − terrain HAE) within terrain-source accuracy.
- Live feed updates render at interactive latency.

## M5 — Point clouds

**Build:** LAS/LAZ loading; GPU-instanced point rendering; chunked spatial subdivision with
per-chunk LOD; georeferenced placement via the same `GeoPose`/frame path.

**Accept when:**
- A multi-million-point LAZ renders interactively and sits correctly on the terrain (a
  ground-classified cloud's floor coincides with the rendered terrain surface within source
  accuracy).

---

## Later / unscheduled

- Region-bundle *export* (cache → PMTiles) for shareable offline packs.
- NAIP high-res imagery layer (US), layer switching UI.
- Adaptive terrain meshing (RTIN/Martini) if triangle budgets bite.
- Out-of-core point-cloud octree for very large clouds.
- EGM2008 fine-grid option, time-dynamic GIBS layers, mission-time playback polish.
