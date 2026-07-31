# Fork Review: Divergence from Upstream `kurtkuehnert/planetary_terrain_renderer`

Date: 2026-07-30
Scope: full diff `upstream/main..main` — 376 files, +15,699 / −747 (src-only: 43 files, +6,116 / −334), ~100 commits.
Method: three parallel code reviews (streaming subsystem, core renderer, support code) cross-checked against the
intended architecture from Kurt Kühnert's bachelor thesis (UDLOD + Chunked Clipmap, read in full), the master-thesis
abstract, and the upstream reference implementation. Upstream has not moved since the fork point, so the entire diff
is local work.

Verification status: `cargo check` passes and all 41 streaming unit tests pass (1 ignored live-network test) —
**but only after removing the private `small_world` dev-dependency** (see §5).

---

## 1. Bottom line

**The fork is worth keeping.** The architecture-level decisions are sound, several fixes are genuinely valuable
(three are upstream-PR material), and nothing warrants revert-to-upstream. The defects are concentrated in the
robustness layer of the streaming scheduler, where the fix-commits repeatedly patched symptoms (log noise, manifest
recovery) instead of causes (infinite retry, write races). That is a bounded, well-localized fix list — repair is
cheaper than reimplementation.

---

## 2. The intended architecture (what the fork must respect)

From the thesis work and upstream code:

1. **Cube-sphere SRS** — the planet is six cube faces; every location is `Coordinate { face, uv }` in f64, using an
   adjusted gnomonic projection (`SIGMA`) to reduce area distortion. The WGS84 ellipsoid enters via `TerrainShape`
   scaling `(a, b, a)`.
2. **Precision strategy** — f64 on CPU only. The GPU never sees absolute world positions: a per-view, per-face
   2nd-order Taylor series (`SurfaceApproximation`) reconstructs surface positions *relative to the viewer* in f32.
   This is the master thesis's core novelty; any shader change that reintroduces face-global f32 arithmetic on hot
   paths violates it.
3. **Geometry (UDLOD)** — a compute prepass (`refine_tiles.wgsl`) subdivides an implicit quadtree per frame by view
   distance, culls, and emits a tile list drawn in a single indirect call; CDLOD-style vertex morphing removes
   cracks/T-junctions; LOD layers blend at ring fringes.
4. **Data (Chunked Clipmap)** — `TileTree` (per-view clipped-quadtree lookup) + `TileAtlas` (shared node storage).
   Tiles load asynchronously; lookups always fall back to the best-loaded ancestor. **The ancestor-fallback machinery
   IS the streaming system**: missing data must degrade gracefully to coarser tiles — never block, never hole.
   Attachment textures carry duplicated borders; filtering uses explicit gradients at tile seams.
5. **Preprocess** — split → downsample → stitch pipeline turning GeoTIFFs into the border-padded tile pyramid.

The fork's streaming layer respects invariant 4: tiles missing local data park in `pending_stream_tiles` *without*
consuming atlas slots, and rendering continues from ancestors. The bugs are in *feeding* the cache, not in the
rendering contract.

---

## 3. Keep unconditionally (genuinely valuable)

| Change | Where | Why |
| --- | --- | --- |
| Ellipsoid-normal height offset | `src/math/terrain_shape.rs:54-60` | Upstream offsets terrain height radially; the fork uses the true geodetic normal `(unit/scale).normalize()`. Upstream is geodetically wrong (km-scale placement error at altitude) and disagreed with its own shader, which already used the inverse-transpose normal. Versioned properly as `geodetic_mapping_version: 2`. **Upstream-PR material.** |
| MSAA depth-copy specialization | `src/render/terrain_pass.rs`, `src/shaders/depth_copy_single.wgsl` | Upstream hardcoded `count: 4` (its own `// Todo`); broken for any non-4×-MSAA camera. Fork specializes for MSAA 1/2/4. **Upstream-PR material.** |
| Atlas slot leak fix + load cancellation | `src/terrain_data/tile_atlas.rs` (~:545), `tile_loader.rs` | Implements upstream's literal `// Todo: cancel loading tiles`; failed asset loads no longer leak slots forever. **Upstream-PR material.** |
| Tile load prioritization | `src/terrain_data/tile_loader.rs` | Replaces upstream's `pop() // Todo`: in-flight attachments first, then request count, then **coarser LOD first** (correct for ancestor fallback), then recency. |
| WGS84 geodesy | `src/math/geodesy.rs` | Numerically verified: constants exact; Bowring (1976) inverse round-trips at ~1e-13° lat / 8e-8 m alt; NED frames correct. Upstream has no lat/lon geodesy at all. See §5 for the vertical-datum caveat. |
| Streaming reprojection core | `src/streaming/terrain_sampling.rs` + provider planning | The hard problem solved right: plan a lat/lon bbox per cube-sphere tile (9-point sampling), fetch one raster, inverse-map every output pixel through the renderer's own cube-sphere → WGS84 math, bilinear-sample. Best-tested code in the module (Alps pinned to correct face/tile at LOD 11 to 1e-6°). |
| Cache-mediated streaming design | `src/streaming/` overall | Disk cache is the only interface between streaming and rendering; GPU pipeline untouched. Correct way to bolt on. |
| Bevy 0.16 → 0.18 migration | mechanical, many files | Verified: no bind-group entries added/removed/reordered anywhere; WGSL face tables and SIGMA warp match Rust tables exactly on all six faces. |
| Tile-tree dirty flag | `src/terrain_data/tile_tree.rs` | GPU buffer re-uploaded only when an entry changed (upstream cloned + re-uploaded every frame). |
| Preprocess modernization | `preprocess/` | gdal 0.19 without `bindgen` (drops the libclang requirement), block-size clamp for small rasters, LOD undersampling warning, `--keep-temp`, self-contained 1.2 MB `sample_data/` tutorial pipeline. Note: deliberate CLI breaking changes (positional → `--long` flags; default format `ru16` → `r16u`). |
| `src/simple.rs` facade | | Clean: registers a small material through the existing generic `TerrainMaterialPlugin<M>`; no forked render logic. (Fix eventually: `terrain_has_albedo` probes CWD-relative `assets/`.) |
| Upstream-Todo cleanups | `attachments.wgsl` relief shading; `format_version` in `terrain.rs` | Cosmetic/robustness, low risk. |

---

## 4. Defect list (keep-with-fixes)

### 4.0 Build-breaker
- **`Cargo.toml`: private SSH dev-dependency `small_world`** (`ssh://git@github.com/Swarm-Command/small_world.git`,
  pinned rev). Cargo resolves root-package dev-deps during ordinary `cargo check`/`cargo metadata`, so anyone
  without access to that private repo cannot build the crate at all. See §5 for disposition.

### 4.1 Streaming scheduler robustness cluster (the epicenter — and why the minimal globe misbehaves)
1. **No failure memory / infinite retry.** `refresh_pending_stream_tiles` (`tile_atlas.rs:485`) re-emits every
   missing attachment every frame; the queue dedups only while in-flight (`scheduler.rs:157`); nothing records
   permanent failure. Antimeridian tiles, OpenTopography area-limit rejections, HTTP errors → re-spawned forever,
   up to 4 tasks/frame, zero backoff. Commits `7bdfa59` ("Reduce … warning noise") and `12ec133` muted/recovered the
   *symptoms* of this loop. **Fix: per-`StreamingRequestKey` failure memo with backoff/permanent states.**
2. **All-or-nothing attachment gating.** A tile leaves `pending_stream_tiles` only when *every* attachment (height
   **and** albedo) exists locally (`tile_atlas.rs:375,485`). Height streams first by priority (`55917b7`) but its
   tiles remain invisible until imagery lands — nothing refines during warm-up, then detail pops in bulk.
   **Fix: per-attachment gating, or render height-complete tiles with fallback/neutral albedo.**
3. **Queue drains finest-LOD-first** (`scheduler.rs:123-134` priority includes deeper-LOD-first), inverting the
   loader's own coarse-first policy and maximizing time-to-first-visible-improvement; it also maximizes the cost of
   defect 6 (sequential ancestor chains). **Fix: coarse-first dequeue.**
4. **Non-atomic cache writes + racy manifest.** `cache_writer.rs:60` writes final paths directly (no temp+rename)
   while the main thread polls `is_file()`; `ensure_registered_source` (`cache_writer.rs:69-129`) does unsynchronized
   read-modify-write of the shared manifest from up to 4 concurrent tasks — the most plausible cause of the very
   "malformed streaming cache manifests" that `12ec133` recovers from. **Fix: temp+rename; serialize or drop the
   manifest.**
5. **OpenTopography nodata wrong twice** (`opentopography.rs:344-351`): (a) any NaN/out-of-range sample rejects the
   whole tile (SRTM void −32768 → coastal tiles permanently fail → retry loop); (b) AW3D30's −9999 nodata *passes*
   the ±20,000 m gate and is cached as a −9,999 m trench. Commit `951b306` hardened decoding but missed nodata
   semantics. **Fix: map nodata to hole/ancestor-fill per sample, not per tile.**
6. **Sequential ancestor-chain fetch, duplicated across siblings** (`scheduler.rs:510-540`): one worker
   blocking-fetches every missing ancestor serially (30-45 s timeouts each); sibling tiles race the same chain
   (TOCTOU on `local_attachment_exists`). Long-blocked `IoTaskPool` threads contend with Bevy's own asset IO.
7. **Fallback asymmetry** (`scheduler.rs:488-507`): locally-derived height fallback triggers only on *pre-flight*
   unavailability, not on fetch/decode failure — failed fetches just loop.
8. **Antimeridian & pole tiles can never stream** (`terrain_sampling.rs:77-81`): lon span >180° is rejected
   ("split-request planning not implemented"). Permanent holes at ±180° and both poles; with defect 1, permanent churn.
9. **Per-frame synchronous `stat()` storm on the main thread** (`tile_atlas.rs:464-507`, `tile_loader.rs`): up to
   4 probes per pending tile per attachment per frame; pending sets reach hundreds at deep-LOD views.
10. **Hardcoded `"assets"` root** in `scheduler.rs:335`, `tile_atlas.rs`, `tile_loader.rs` — breaks packaged
    assets, wasm, custom asset roots, non-CWD launches; downloads write into the source `assets/` tree.
11. **Stuck-tile edge case**: albedo cached + height missing + `stream_height=false` → height request dropped by
    policy (`scheduler.rs:260-269`), no imagery request to trigger derived-height → pending forever.
12. Hygiene: dead freshness/expiry/hash machinery in `cache_manifest.rs:138-166` (write-only); test-only
    `LocalTileSource` trait; unused `Sentinel2Cog`; `"albedo"` magic string in ≥5 places; triplicated bilinear
    samplers and TIFF encoders; two height-TIFF decoders accepting *different* sample types (`scheduler.rs:832` vs
    `opentopography.rs:290`); blank-tile heuristic (`gibs.rs:399-416`) refetches genuinely uniform ocean/ice tiles;
    queue eviction uncounted in stats; a unit test writes into the repo `assets/` (cleanup only on success).

### 4.2 Core renderer
13. **`is_upload_tile_relevant` drops deferred uploads of cached tiles** (`tile_atlas.rs:208` + `gpu_tile_atlas.rs`
    upload budget): requires `requests > 0`, but a tile released while its upload is budget-deferred stays `Loaded`
    (cached); on re-request the atlas slot serves the **previous occupant's texels**. **Fix: check slot identity
    (`atlas_index`) only.** The 24 MiB/frame upload budget itself is fine once this is fixed.
14. **`coordinate_change_lod` f32 round trip** (`functions.wgsl:215,236`; commit `00c8bc6`): the cross-face LOD seam
    fix is real and the math matches the Rust tables exactly on all six faces — but it replaced *exact* pow-2
    integer/fraction arithmetic with a face-global f32 cube-sphere round trip on the hottest fragment path
    (`lookup_best` calls it up to `lod_count` times per fragment). ~2⁻²³ face-relative error ≈ 1.2 m on Earth →
    texel-quantized lookups around LOD 14-16, plus heavy ALU. This is the error class the Taylor-series design
    exists to eliminate. **Fix: keep exact arithmetic for same-face rescale; take the round trip only when the
    scaled uv leaves [0,1] (cross-face case).**
15. `perf.rs`: per-phase sample `Vec`s grow unbounded while telemetry is enabled (`perf.rs:58-69`). Cap or ring-buffer.
    Keep only if the `spherical_multires` benchmark harness stays (it is the only structured consumer).
16. `geodesy.rs:124`: `hae = p/cos(lat) − nu` is 0/0 at the exact poles; use `h = p·cosφ + z·sinφ − a·W`. Bowring
    single-pass degrades at satellite altitudes — fine for terrain, worth a doc comment.

### 4.3 Why the minimal globe is "not working as desired" (commit `2f9b58c`)
The commit moved the default view to ~10 km over Everest and raised `DEFAULT_MAX_LOD` 7 → 10, while bundled data
covers only LODs 0-2. Everything from LOD 3-10 must stream, which stresses defects 1, 2, 3, 5, 8, 9
simultaneously: height fills the cache invisibly (gating), coarse fallback tiles are rejected by OpenTopography's
area limit and retried forever (no failure memory), fine tiles beat their ancestors through the queue
(finest-first), and hundreds of pending tiles stat the filesystem every frame. Highest-leverage fixes, in order:
failure memo + backoff; per-attachment gating; coarse-first dequeue; then atomic writes and nodata mapping.

---

## 5. `small_world` and the vertical-datum question

Context: `small_world` is a private in-house geodetic transformation library (LLA WGS84/MSL, ECEF, NED/ENU, …).
The renderer must display the reported state of autonomous agents as realistically as possible, and **HAE vs MSL vs
AGL produce very different results** — the vertical reference system must be well-defined end-to-end. In this fork
`small_world` is used only as a *test oracle* for `geodesy.rs` (6 of 10 tests), but as a private SSH git dev-dep it
breaks `cargo check` for everyone else.

Current state of the fork's own geodesy: **`geodesy.rs` handles HAE only** (ellipsoid heights). It has no geoid
model, so it cannot express MSL; AGL additionally requires querying the renderer's own terrain height. The
OpenTopography DEMs being streamed (SRTM/AW3D30) are themselves **EGM96-orthometric (≈MSL) heights**, currently
treated as if they were ellipsoidal — a systematic ±~100 m class error (geoid undulation) that matters exactly for
the stated use case.

Disposition options:
1. **Short-term (unblock builds):** feature-gate the oracle tests (e.g. `#[cfg(feature = "small_world_oracle")]`
   with the dep behind the same optional feature) or replace the oracle with hardcoded expected values captured
   once from `small_world`. Keeps the 4 self-contained round-trip tests running for everyone.
2. **Recommended replacement stack (empirically verified 2026-07-30, all-Rust, wasm-compatible, no PROJ):**
   - **Frames** (LLA-HAE ↔ ECEF ↔ NED/ENU): keep the fork's own `geodesy.rs` — already verified correct
     (round-trips at ~1e-13° / 8e-8 m). Alternatives (`nav-types`, `map_3d`) add nothing; both are HAE-only too.
   - **HAE ↔ MSL**: the **`egm96` crate v0.2.3** (Zlib license, micahcc/egm96-rs). Validated in-sandbox against
     NGA reference undulations:
     | Location | Reference | SH eval | 5′ raster |
     | --- | --- | --- | --- |
     | (0°, 0°) | +17.16 m | 17.162 | 17.150 |
     | Everest (27.9881°N, 86.925°E) | −28.74 m | −28.741 | −29.305 |
     | Indian Ocean low (4.75°N, 78.75°E) | ≈−107 m | −106.991 | −107.012 |
     Spherical-harmonic path matches to millimeters at ~0.7 ms/call; the embedded 5′ raster is within ~0.5 m even
     in the worst geoid terrain at ~1 µs/lookup (negligible vs SRTM's ~16 m LE90). Data cost: 1.3 MB (15′ grid)
     or 8 MB (5′ grid) PNG.
     **Adoption caveat:** the crate's default `fetch-maps` feature downloads the grid PNGs from the author's
     personal GitHub Pages *at build time* — a supply-chain/CI-reproducibility hazard. Use
     `default-features = false, features = ["raster_5_min"]`, vendor the two PNGs into the repo, and point
     `EGM96_5_MIN`/`EGM96_15_MIN` at them via `[env]` in `.cargo/config.toml` (verified: builds clean offline).
   - **Do NOT use the `egm2008` crate**: it embeds only a 3-degree grid — measured **16 m error at Everest**
     (−44.687 vs −28.74). Unusable for this purpose.
   - Heavier alternatives if ever needed: Rust Geodesy (`geodesy` crate, PROJ-like pipelines) or `proj` bindings
     (proper vertical datum grids incl. EGM2008, but a C dependency, incompatible with wasm).
   - **AGL** should be answered by the renderer itself (terrain-height query at the agent's footprint — the
     thesis's "random-access terrain data" requirement exists for exactly this), not by a geodesy library.
3. Whatever the choice, fix the streamed-DEM datum handling (orthometric → ellipsoidal at ingest) at the same time,
   since it is the same geoid model doing the work. The geoid is smooth at tile scale: evaluating undulation at
   tile corners and bilinearly interpolating is sufficient and effectively free.

---

## 6. Discard / relocate

- `skills/terrain-*` (all 9 packages) — AI-agent orchestration artifacts for the fork's own development process;
  several presuppose `small_world` access; zero value to library users. Move out of the repo if the workflows are
  still wanted.
- Docs that are dated AI planning residue: `agent-orchestration-plan-2026-03-07.md`,
  `physical_truth_orchestration_plan_2026-03-07.md`, `earth_streaming_orchestration_plan_2026-03-09.md`,
  `earth_streaming_tier1_packets_2026-03-09.md`, `physical_truth_findings_2026-03-07.md`,
  `performance_findings_2026-03-07.md`.
- Docs worth keeping: `getting_started.md`, `multires_workflow.md`, `imagery_lod_seam_fix.md` (real root-cause
  analysis of defect 14's underlying bug), `physical_truth_mapping_audit_2026-03-07.md` (documents the two WGS84
  fixes; the justification record for `geodesy.rs`), `earth_streaming_cache_contract_2026-03-09.md` (specifies the
  on-disk format `cache_manifest.rs` implements), `performance_benchmarking.md` (iff the benchmark harness stays).
  Several contain hardcoded absolute paths from the original author's machine — strip them.
- `examples/spherical_multires*` (2.2k lines) — a benchmark/inspection harness wearing an example costume; keep only
  if the benchmarking workflow is valued, else discard together with `perf.rs` and its ~10 instrumentation call-sites.
- Dead streaming machinery listed in defect 12.

---

## 7. Suggested fix order

1. `small_world` dev-dep → feature-gate or hardcoded oracle values (unblocks every other contributor and CI).
2. Failure memo + backoff in the streaming scheduler (defect 1) — stops the retry storm and the log spam at the source.
3. Per-attachment gating / height-with-fallback-albedo (defect 2) — makes streaming progress visible.
4. Coarse-first dequeue (defect 3) — aligns queue with the ancestor-fallback design.
5. Atomic cache writes + manifest serialization (defect 4).
6. Nodata mapping for OpenTopography (defect 5) — and the HAE/MSL ingest fix (§5.3) while in there.
7. `is_upload_tile_relevant` slot-identity check (defect 13).
8. Restrict `coordinate_change_lod` round trip to the cross-face case (defect 14).
9. Async/cached existence probing + asset-root plumbing (defects 9, 10).
10. Hygiene pass (defect 12) and docs/skills cleanup (§6).
