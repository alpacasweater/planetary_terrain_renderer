# heimdall

> Heimdall, the all-seeing: he watches everything, everywhere, exactly where it really is.

**heimdall** is a georeferenced real-world asset visualizer. It streams free terrain and satellite
imagery onto a WGS84 globe and places real-world autonomous assets — positions, tracks, point
clouds — with survey-correct vertical references (HAE / MSL / AGL / ECEF), in real time. Explore a
region while online; keep working with it fully offline.

Built in Rust on [Bevy](https://bevy.org), with
[`small_world`](https://github.com/Swarm-Command/small_world) as the single source of truth for all
reference-frame and vertical-datum math.

## Status

**Pre-M0.** Planning, documentation, and the geodesy/tile-math core are in place; rendering lands
with milestone M0. See [`docs/roadmap.md`](docs/roadmap.md).

## Why this exists

Predecessor work lived in a fork of `planetary_terrain_renderer`, a thesis-grade planetary
renderer. Auditing and repairing it taught us that most of its complexity (cube-sphere SRS,
Taylor-series GPU precision, bespoke preprocessing) served a research goal we don't share, while
the part we *do* need — correct georeferencing — is independent of all of it. heimdall is the
clean rebuild: standards-based tiling, floating-origin precision, and a fraction of the code.
The full rationale and architecture live in [`docs/design.md`](docs/design.md).

## Core design decisions

- **Tiling projection ≠ positioning frame.** Data is fetched in the WebMercator (slippy-map)
  quadtree — the scheme all free terrain/imagery is published in — but geometry and assets live on
  the true WGS84 ellipsoid in ECEF. Mercator is an index, never a coordinate frame.
- **One geodesy authority.** Every LLA/MSL/AGL/ECEF/ENU/NED conversion goes through `small_world`.
  The per-interface vertical-datum contract is [`docs/reference_frames.md`](docs/reference_frames.md).
- **Floating-origin precision** via `big_space` (from M0), not per-view Taylor expansion.
- **Free data, cacheable by license.** Terrain: Mapterhorn (Copernicus DEM 30 m, Terrain-RGB
  PMTiles). Imagery: EOX Sentinel-2 cloudless (~10 m) + NASA GIBS. Sources whose ToS forbid
  offline caching (Google/Esri/Bing/Mapbox) are deliberately excluded.
- **Offline-first caching.** SQLite (MBTiles schema) runtime cache + PMTiles region bundles +
  a `prefetch(bbox, zooms)` command.

## Layout

```
docs/
  design.md            architecture, data sources, licensing, risks
  reference_frames.md  the vertical-datum contract (normative)
  roadmap.md           milestones M0–M5 with acceptance criteria
src/
  lib.rs
  frame.rs             geodesy facade over small_world (frames & datums)
  tiles.rs             WebMercator quadtree math (z/x/y ↔ lon/lat)
```

## Building

Requires access to the private `small_world` repo (the Cargo dependency is fetched over HTTPS).

```sh
cargo test
```

CI (fmt, clippy `-D warnings`, tests) needs a repo secret `SMALL_WORLD_TOKEN` — a fine-grained PAT
with read access to `Swarm-Command/small_world` — so Cargo can fetch the private dependency.

## License

Private, internal. Data sources retain their own licenses (see `docs/design.md` §4 — note the
Sentinel-2 cloudless non-commercial terms).
