# Earth Streaming And Offline Cache Plan (2026-03-09)

## Mission
Bring `planetary_terrain_renderer` to a state where it provides:
- an instant offline Earth experience from a bundled starter dataset
- optional online refinement for height and imagery
- durable local caching so later sessions work offline
- one local-first runtime data path instead of separate offline and online render paths

## Product Contract
- The repo continues to ship a bundled low-resolution Earth under `assets/terrains/earth`.
- The renderer remains local-first. Remote sources only materialize missing tiles into a writable cache.
- Height and imagery can be streamed independently.
- Offline mode must never issue network requests.
- If a remote tile is unavailable, the renderer falls back to the bundled starter tile or a parent LOD.
- Remote refinement must preserve current tile border and mip semantics so seams do not regress.

## Why This Shape Fits The Current Code
Current constraints make a cache-backed design the lowest-risk path:
- the tile loader currently reads local attachment TIFFs through the Bevy asset server in `src/terrain_data/tile_loader.rs`
- tile paths are derived from the current terrain tile layout in `src/math/coordinate.rs`
- base Earth requests are gated by `existing_tiles` in `src/terrain_data/tile_atlas.rs`
- the current material path already supports height-only or albedo-backed rendering, so this is primarily a data-plane change rather than a shading rewrite

The main architectural change required is to make base Earth tile availability procedural up to a configured maximum LOD instead of relying on a fully enumerated `config.tiles` list.

## Preferred Source Stack
### Imagery
- Tier-1 global fallback: NASA GIBS
- Tier-2 regional improvement: Sentinel-2 via STAC/COG discovery and direct cloud assets
- Tier-3 optional local high-resolution overlays: NAIP for U.S. regions

### Height
- Tier-1 bundled global fallback: current low-resolution base Earth height dataset
- Tier-2 online land refinement: OpenTopography-backed DEM fetches
- Tier-3 future source expansion: higher-quality or provider-specific DEM backends once cache semantics are stable

## Shared Gates
- `cargo check --workspace` remains green
- bundled `cargo run --example minimal_globe` remains the default zero-network path
- streamed tiles are written into the same on-disk attachment layout the renderer already knows how to load
- online refinement never regresses offline startup or offline replay
- border overlap and mip continuity remain numerically seam-safe for streamed tiles
- network failures degrade to fallback tiles rather than panics or blank terrain

## Tiered Execution
Each tier is sequential. Tasks inside a tier are intended to be executed in parallel.
A later tier should not begin until the current tier's acceptance criteria are satisfied.

## Tier 1: Foundations
Parallel objective: create the local-first cache architecture and remove the manifest limitations that block online refinement.

| Task | Focus | Skill | Branch suggestion | Depends on | Deliverables |
|---|---|---|---|---|---|
| E1 | Cache-backed tile source abstraction | `terrain-streaming-optimizer` | `codex/earth-cache-source` | none | local-first tile source interface, cache root config, starter-vs-cache read order |
| E2 | Procedural base Earth tile availability | `terrain-raster-truth` | `codex/earth-procedural-tiles` | none | base Earth can request tiles up to configured max LOD without enumerating every tile in `config.tiles` |
| E3 | Source contract and cache metadata schema | `terrain-raster-truth` | `codex/earth-cache-schema` | none | provider abstraction, cache manifest schema, source metadata format, cache invalidation policy |

### Tier 1 Ownership Boundaries
- E1 owns loader, cache lookup, and request lifecycle changes.
- E2 owns terrain config semantics and base Earth tile-existence logic.
- E3 owns provider-facing interfaces, metadata files, and cache versioning.

### Tier 1 Acceptance
- Existing starter Earth still renders unchanged.
- Cache and starter datasets can coexist without changing the render path.
- Base Earth can legally request higher LODs than the bundled starter dataset currently contains.
- There is a written cache schema that both imagery and height implementations can share.

## Tier 2: First Online Data Paths
Parallel objective: implement the first real cache-filling backends using the Tier 1 interfaces.

| Task | Focus | Skill | Branch suggestion | Depends on | Deliverables |
|---|---|---|---|---|---|
| E4 | Global imagery cache fill via NASA GIBS | `terrain-raster-truth` | `codex/earth-gibs-imagery` | E1, E2, E3 | remote imagery fetch, reprojection to terrain tiles, border fill, mip generation, cache writes |
| E5 | Land height cache fill via OpenTopography | `terrain-geodesy-truth` | `codex/earth-height-streaming` | E1, E2, E3 | remote DEM fetch, reprojection to terrain tiles, cache writes, bundled base fallback |
| E6 | Runtime scheduler, policy, and cancellation | `terrain-streaming-optimizer` | `codex/earth-stream-scheduler` | E1, E2, E3 | background job scheduling, inflight priority, cancellation, retry/backoff, offline-only and per-source enable flags |

### Tier 2 Ownership Boundaries
- E4 owns imagery provider logic and imagery-specific cache population.
- E5 owns height provider logic and terrain/elevation correctness.
- E6 owns runtime orchestration, job prioritization, and online/offline behavior.

### Tier 2 Acceptance
- Moving the camera online populates higher-detail imagery tiles into cache.
- Height refinement is available over supported land regions and falls back cleanly elsewhere.
- Restarting offline reuses the warmed cache without network access.
- Network failures leave visible fallback terrain instead of empty regions.

## Tier 3: Productization And Safety
Parallel objective: make the feature understandable, controllable, and verifiable for users.

| Task | Focus | Skill | Branch suggestion | Depends on | Deliverables |
|---|---|---|---|---|---|
| E7 | User-facing settings, examples, and docs | `terrain-build-sheriff` | `codex/earth-streaming-ux` | E4, E5, E6 | config knobs, example integration, docs for starter/offline/online modes |
| E8 | Cache tools and observability | `terrain-streaming-optimizer` | `codex/earth-cache-tooling` | E4, E5, E6 | cache inspection, cleanup, cache size cap, runtime stats overlay/logging |
| E9 | Correctness, seam, and release validation | `terrain-release-verifier` | `codex/earth-streaming-verify` | E4, E5, E6 | validation matrix, seam checks, offline replay checks, benchmark safety gate |

### Tier 3 Acceptance
- A new user can understand when the app is offline-only, warming cache, or serving cached data.
- Users can clear or cap cache without corrupting the terrain state.
- Streaming passes seam, fallback, and offline replay checks.
- Documentation clearly separates bundled starter behavior from optional online refinement.

## Tier 4: Source Expansion And Tuning
Parallel objective: improve data quality and operational robustness after the first end-to-end path is stable.

| Task | Focus | Skill | Branch suggestion | Depends on | Deliverables |
|---|---|---|---|---|---|
| E10 | Higher-quality regional imagery sources | `terrain-raster-truth` | `codex/earth-imagery-expansion` | E7, E8, E9 | Sentinel-2 and other source ranking logic, regional override policy |
| E11 | Additional DEM backends and quality policy | `terrain-geodesy-truth` | `codex/earth-height-expansion` | E7, E8, E9 | expanded DEM provider support, coverage policy, source selection rules |
| E12 | Performance tuning on warmed-cache scenarios | `terrain-benchmark-profiler` | `codex/earth-streaming-bench` | E7, E8, E9 | benchmark artifacts for cold-cache vs warm-cache behavior, upload pressure analysis |

### Tier 4 Acceptance
- Better providers can override the baseline source stack without changing the local cache contract.
- Warm-cache performance remains acceptable and benchmarked.
- Source ranking rules are explicit and documented.

## Execution Order
1. Execute E1, E2, and E3 in parallel.
2. Do not begin E4, E5, or E6 until Tier 1 cache interfaces and procedural tile-availability semantics are stable enough to avoid parallel rework.
3. Execute E4, E5, and E6 in parallel once Tier 1 lands.
4. Hold E7, E8, and E9 until the Tier 2 backends have stable config knobs and cache behavior.
5. Execute E10, E11, and E12 only after the first end-to-end online/offline loop is validated.

## Detailed Tier 1 Packet
For the first implementation wave, use [Tier 1 Work Packets](earth_streaming_tier1_packets_2026-03-09.md) as the concrete handoff document for parallel execution.

## Recommended Initial Scope
The first release target should be:
- bundled low-resolution Earth height and imagery in-repo
- online imagery cache fill via NASA GIBS
- online land-height refinement via OpenTopography
- offline replay from local cache

Do not start with direct runtime rendering from network responses.
Do not introduce separate online-only shader or atlas logic.
Do not block the starter experience on network access, authentication, or provider availability.

## Key Risks
- the current `existing_tiles` gate in `TileAtlas` prevents online refinement unless base Earth tile-existence semantics are changed
- remote imagery and DEM products do not naturally match the renderer's cube-face tile boundaries, so border generation and mip continuity must be validated carefully
- provider rate limits, uptime, and attribution requirements can easily leak into the runtime UX if the cache layer is not explicit
- height coverage is likely to be uneven across the globe, so bundled low-resolution fallback must remain permanent rather than temporary
- cold-cache network fetches can create upload bursts unless the scheduler and upload budget rules are aligned

## Validation Matrix
Every end-to-end milestone should rerun at least:
- bundled offline startup: `cargo run --example minimal_globe`
- bundled starter plus online imagery warmup
- bundled starter plus online height warmup over a supported land region
- offline replay using warmed cache with network disabled
- seam continuity checks on newly cached height and albedo tiles
- benchmark comparison for cold-cache and warm-cache camera motion

## Suggested Repository Touch Points
Likely hot files and modules for the first three tiers:
- `src/terrain_data/tile_loader.rs`
- `src/terrain_data/tile_atlas.rs`
- `src/plugin.rs`
- `src/terrain.rs`
- `examples/minimal_globe.rs`
- new `src/streaming/*` modules for provider integrations and cache writing
- docs and quick-start material for explicit online/offline behavior

## Definition Of Done For The Feature Family
This initiative is complete when:
- the repo ships an offline Earth that works from a clean clone
- an online user can warm a cache for higher-quality imagery and land height
- later launches can replay those warmed regions offline
- the runtime still behaves as one coherent terrain system instead of separate offline and online code paths
