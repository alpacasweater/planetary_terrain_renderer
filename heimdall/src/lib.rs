//! # heimdall
//!
//! Georeferenced real-world asset visualization: streamed terrain + satellite imagery on a WGS84
//! globe, with survey-correct HAE/MSL/AGL/ECEF placement of assets, tracks, and point clouds.
//!
//! Architecture (see `docs/design.md`): five layers — `frame` (geodesy authority), `tiles`
//! (WebMercator quadtree + fetch/decode/cache), `render` (globe LOD), `assets` (geo-posed
//! overlays), `ingest` (feeds). The two foundational, dependency-light layers live here first;
//! the rest land milestone by milestone (`docs/roadmap.md`).
//!
//! ## The one rule
//!
//! **Tiling projection ≠ positioning frame.** Data is indexed in the WebMercator quadtree
//! ([`tiles`]); geometry and assets live on the true WGS84 ellipsoid in ECEF ([`frame`]).
//! Mercator never carries a position; `small_world` carries them all.

pub mod frame;
pub mod tiles;
