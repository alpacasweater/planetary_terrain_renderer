# Handoff: Geodesy Integration & Cleanup Session

Audience: the next working session, which will have **both** `planetary_terrain_renderer` and
`Swarm-Command/small_world` as sources.
Prepared: 2026-07-31, from the session that produced `docs/fork_review_2026-07-30.md`.

## Read first

1. `docs/fork_review_2026-07-30.md` — full audit of this fork vs upstream: what to keep, the 16-item defect
   list with file:line refs, why the minimal globe misbehaves, and the prioritized fix order (§7). This handoff
   does not repeat that content.
2. §2 of that doc — the upstream architecture invariants (cube-sphere SRS, Taylor-series GPU precision,
   ancestor-fallback atlas). Every change must respect them.

## Mission

**Make `small_world` the single source of truth for all reference-frame transformations** — LLA (HAE and MSL),
ECEF, NED/ENU, AGL — and fix the defects from the review. The renderer must display reported autonomous-agent
state as realistically as possible; HAE vs MSL vs AGL produce very different results, so the vertical reference
of every interface must be explicit and well-defined.

### small_world integration plan

1. **Unblock the dependency.** Change the dep URL from `ssh://git@github.com/...` to
   `https://github.com/Swarm-Command/small_world.git` (same pinned rev) — remote sessions and CI authenticate
   over HTTPS, not SSH. Note: the existing `quickstart.yml` CI almost certainly fails at dependency resolution
   for the same reason (cargo resolves dev-deps during `cargo check`); verify and fix while in there.
2. **Promote small_world from test oracle to runtime geodesy provider.** Today it is a dev-dep used only to
   cross-check `src/math/geodesy.rs`. Single-source-of-truth means the renderer's own duplicated math goes away:
   - Replace the internals of `geodesy.rs` (LLA↔ECEF, NED frames) with calls into small_world, or delete it and
     re-export small_world types behind a thin renderer-facing module. Keep one adapter seam
     (`src/math/geodesy.rs` as facade) so renderer code has a single import path and the ECEF→renderer axis swap
     `(x,y,z)→(−x,z,y)` lives in exactly one place.
   - The `terrain_shape.rs` ellipsoid-normal fix and the cube-sphere `Coordinate` math are renderer-internal
     (thesis SRS, not geodesy) — they stay.
   - Decision to make deliberately: a hard runtime dep on a private crate makes this repo unbuildable outside
     the org. If that matters, hide the provider behind a small trait with small_world as the default (workspace)
     implementation; if it doesn't, take the hard dep and simplify.
3. **Define the vertical-datum contract in one place.** A short `docs/reference_frames.md` (or module doc on the
   facade) stating, per interface: agent state ingest (HAE or MSL — pick and enforce), terrain tile heights
   (ellipsoidal, post-ingest), AGL (renderer terrain query at the agent footprint — the thesis's random-access
   terrain-data requirement exists for exactly this; small_world cannot answer it).
4. **Fix the streamed-DEM datum bug at ingest** (fork_review §5.3): OpenTopography SRTM/AW3D30 heights are
   EGM96-orthometric (≈MSL) but are currently cached as if ellipsoidal — a ±~100 m class systematic error. Convert
   orthometric → ellipsoidal at tile-write time using small_world's geoid. The geoid is smooth at tile scale:
   evaluate undulation at tile corners and bilinearly interpolate.
5. **Validate small_world's geoid before trusting it.** Reference undulations (NGA EGM96), already used to
   validate the `egm96` crate in-sandbox (fork_review §5.2): (0°N, 0°E) → +17.16 m; Everest (27.9881°N,
   86.925°E) → −28.74 m; Indian Ocean low (4.75°N, 78.75°E) → ≈−107 m. If small_world matches to sub-meter,
   proceed; if it lacks a geoid or is coarse, the verified fallback is the `egm96` crate v0.2.3 (Zlib) with
   vendored grids — full recipe and caveats in fork_review §5.2.
6. **Keep the tests.** The 4 self-contained geodesy round-trip tests survive any refactor; the 6 oracle-comparison
   tests become ordinary tests once small_world is a real dependency.

### Then the defect fix order

Follow `fork_review_2026-07-30.md` §7. Condensed: (1) small_world dep fixed as above → builds unblocked;
(2) streaming failure memo + backoff; (3) per-attachment gating; (4) coarse-first dequeue; (5) atomic cache
writes + manifest serialization; (6) OpenTopography nodata mapping — do together with the datum fix, same code
path; (7) `is_upload_tile_relevant` slot-identity check; (8) restrict the `coordinate_change_lod` f32 round trip
to the cross-face case; (9) async existence probing + asset-root plumbing; (10) hygiene.

## Streamlining audit — lean, concise, efficient, correct

Observations beyond the defect list, aimed at shrinking the project's surface area.

### Lean (dependency & feature structure)
- **Gate streaming behind a cargo feature.** `ureq` is currently an unconditional dependency and streaming
  systems always register. A `streaming` feature (off → core renderer + local datasets only) keeps the base
  crate light, drops the HTTP stack for offline users, and shrinks wasm builds.
- **Audit `Cargo.toml`:** `libc` in a Bevy plugin is suspicious — find the single use and likely delete;
  `ndarray` appears in both root and preprocess — confirm the root crate really needs it; `image` already has
  default features off (good) — check `tiff` crate vs `image`'s tiff feature for redundancy.
- **Move the benchmark harness out of `examples/`.** `spherical_multires*` (2.2k lines) is a dev tool wearing
  an example costume, and `perf.rs` + its ~10 instrumentation call-sites exist only to feed it. Either a
  separate workspace member (`tools/bench/`) or a `bench-telemetry` feature; core render code then carries zero
  perf-counter noise. Fix perf.rs's unbounded sample growth if kept.
- **Make `minimal_globe` minimal.** ~250 of its 421 lines are env-var/CLI plumbing (8 env vars + flags). The
  copyable example should be ~100 lines; move knobs to one `ExampleArgs` helper shared by the demo examples, or
  cut them.
- Delete `skills/` and the six AI-planning-residue docs (list in fork_review §6); strip the author-machine
  absolute paths from the docs that stay.

### Concise (dedupe & dead code)
- One `streaming/util.rs` for the **triplicated** bilinear samplers (`scheduler.rs:802`,
  `opentopography.rs:482`, `gibs.rs:418`) and TIFF encoders (×3); **unify the two height-TIFF decoders that
  accept different sample types** (`scheduler.rs:832` vs `opentopography.rs:290`) — that divergence is a latent
  bug, not just duplication.
- Replace the `"albedo"` magic string (≥5 sites) with a constant or attachment-label enum; any custom
  attachment silently cannot stream today.
- Delete write-only machinery: cache freshness policy / expiry / content-hash fields (`cache_manifest.rs:138-166`),
  test-only `LocalTileSource` trait, unused `Sentinel2Cog` variant, never-produced `Focused` priority (or wire
  it to the camera focus, which was clearly the intent).
- `tile_atlas.rs` has three concerns interleaved (residency, streaming gate, telemetry) — after the fixes,
  split the streaming gate into its own module so the atlas file returns to upstream-diffable shape.

### Efficient
- Replace per-frame `stat()` polling with event-driven promotion: the cache writer already knows when a tile
  lands — send a message (async-channel is already a dep) instead of re-statting hundreds of paths per frame.
- `dequeue_batch` drains, sorts, and reinserts the whole pending map every frame (`scheduler.rs:217-244`) —
  use a proper priority structure (`BinaryHeap` + generation stamps or `priority-queue` crate).
- Shader: after the cross-face fix (defect 14), `lookup_best`'s per-fragment cost drops back to upstream levels —
  re-run the benchmark harness before/after to confirm no regression remains.
- Tile-tree dirty-flag pattern (already added) is the model: look for the same clone-and-reupload-every-frame
  pattern elsewhere in the render extract path.

### Correct (guardrails so it stays correct)
- **CI is the gap.** `quickstart.yml` only runs `cargo check` + example checks. Add: `cargo test` (unit tests
  exist and are decent — 41 streaming + geodesy + terrain config), `cargo clippy -- -D warnings`, `cargo fmt
  --check`. The review found zero *compile*-visible defects — everything was semantic — so tests are the only
  net that catches this class of regression.
- Add a datum round-trip regression test: synthetic DEM tile in → cached tile out, assert ellipsoidal offset
  equals small_world's undulation at the tile center.
- Add one streaming integration test with a mock `TileStreamingSource` (trait exists in `source_contract.rs`)
  exercising: failure → backoff → no re-request; partial attachments → per-attachment promotion; antimeridian
  tile → deterministic unavailable (not retry-forever).
- Property tests for `Coordinate` ↔ unit-position ↔ lat/lon round trips across all six faces (the review
  hand-verified the tables once; a proptest pins them forever).
- The versioning pattern (`format_version`, `geodetic_mapping_version`) is good — bump `geodetic_mapping_version`
  when the datum ingest fix lands, since cached tiles change meaning; add a manifest check that refuses to mix
  versions in one cache.
