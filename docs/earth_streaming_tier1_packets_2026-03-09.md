# Earth Streaming Tier 1 Work Packets (2026-03-09)

This document turns Tier 1 of the Earth streaming plan into concrete parallel packets.

Tier 1 goal:
- create a local-first cache architecture
- remove the `config.tiles` limitation for base Earth refinement
- define the shared cache/source contract before any provider-specific implementation starts

Tier 1 packets are intended to run in parallel:
- E1: cache-backed tile source abstraction
- E2: procedural base Earth tile availability
- E3: cache metadata and provider contract

## Shared Coordination Rules
- Preserve the current zero-network starter path: `cargo run --example minimal_globe`
- Do not add provider-specific HTTP logic in Tier 1
- Do not change terrain shader behavior in Tier 1
- Treat the on-disk terrain tile layout as stable unless explicitly coordinated across all three packets
- If one packet needs a shared type or trait in a new module, that module must be narrow and provider-agnostic

## Integration Contract
Before Tier 2 starts, Tier 1 should agree on:
- cache root selection and override rules
- how a tile request is resolved against `cache -> bundled starter -> parent fallback`
- whether a requested base Earth tile is considered valid even if it is not listed in `config.tiles`
- how cache metadata records source provenance, freshness, and format version

## Packet E1
### Goal
Introduce a cache-aware, local-first tile source abstraction without changing user-visible rendering behavior.

### Scope
- replace the hard-coded loader assumption that every request maps directly to a single local asset path
- support layered local resolution:
  - streamed cache first
  - bundled starter dataset second
  - existing parent-LOD fallback unchanged

### Primary files
- `src/terrain_data/tile_loader.rs`
- `src/terrain_data/tile_atlas.rs`
- `src/plugin.rs`
- `src/lib.rs`

### Suggested new modules
- `src/streaming/mod.rs`
- `src/streaming/tile_source.rs`
- `src/streaming/cache_paths.rs`

### Concrete tasks
1. Define a provider-agnostic local tile source interface that resolves a requested attachment tile into a concrete readable asset path when available.
2. Add terrain/cache settings for:
   - cache root
   - bundled starter root
   - local-first resolution order
3. Refactor the default loader to ask the tile source layer for a resolved local tile before calling `asset_server.load(...)`.
4. Keep current TIFF loading semantics unchanged once a path is resolved.
5. Add debug logging or counters that distinguish:
   - cache hit
   - starter hit
   - unresolved local miss

### Non-goals
- no HTTP fetching
- no cache writing
- no provider-specific metadata parsing

### Acceptance
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo run --example minimal_globe`
- starter Earth still renders without any cache directory present

### Handoff needed by Tier 2
- a stable way for remote workers to write a tile into cache such that the loader will pick it up on the next request

## Packet E2
### Goal
Make base Earth tile availability procedural up to a configured maximum LOD so online refinement is not blocked by `config.tiles`.

### Scope
- remove the requirement that every future base-Earth tile be pre-enumerated in `config.tiles`
- preserve explicit tile lists for overlays and non-procedural terrains

### Primary files
- `src/terrain.rs`
- `src/terrain_data/tile_atlas.rs`
- `src/math/coordinate.rs`
- any terrain config serialization/deserialization touched by new semantics

### Concrete tasks
1. Introduce terrain config semantics that can represent:
   - explicit tile list behavior for existing terrains
   - procedural full-face availability for base Earth up to a `max_lod`
2. Update request gating in `TileAtlas` so base Earth requests are allowed when they fall inside the procedural envelope.
3. Preserve current behavior for overlays such as `swiss` and `los`.
4. Ensure parent fallback logic still works correctly when a requested child tile does not yet exist locally.
5. Add tests for:
   - explicit tile-list terrains
   - procedural base-Earth availability
   - invalid out-of-range tile requests

### Non-goals
- no network logic
- no imagery/height source selection
- no cache writing

### Acceptance
- `cargo test --workspace`
- bundled Earth still renders
- requesting higher base LODs no longer depends on a fully enumerated `config.tiles`

### Handoff needed by Tier 2
- a deterministic answer to: “is this base Earth tile request valid and worth scheduling?”

## Packet E3
### Goal
Define the cache schema and provider contract that Tier 2 online backends must implement.

### Scope
- versioned cache metadata
- source provenance
- attachment/source compatibility rules
- invalidation and freshness semantics

### Primary files
- `docs/earth_streaming_orchestration_plan_2026-03-09.md`
- `docs/earth_streaming_cache_contract_2026-03-09.md`
- new schema docs under `docs/`
- new source-contract modules under `src/streaming/`

### Suggested new modules
- `src/streaming/source_contract.rs`
- `src/streaming/cache_manifest.rs`

### Concrete tasks
1. Define cache metadata fields for at least:
   - source kind
   - source identifier
   - attachment label
   - terrain path
   - cache format version
   - fetch timestamp
   - reprojection or source-zoom inputs
2. Specify how cached attachment tiles are marked usable or stale.
3. Specify the minimum interface that a future remote provider must satisfy:
   - declare attachment type
   - answer availability for a terrain tile request
   - materialize a local cache tile
4. Keep the contract generic enough for:
   - imagery providers like NASA GIBS
   - DEM providers like OpenTopography
5. Document cache invalidation rules for:
   - schema version changes
   - provider changes
   - optional freshness expiry

### Non-goals
- no runtime fetch implementation
- no scheduler
- no UI/settings work beyond contract needs

### Acceptance
- there is a written and reviewable cache schema document
- there is a narrow provider contract suitable for both imagery and height
- E1 and E2 can integrate against the contract without provider-specific code

### Handoff needed by Tier 2
- a stable, versioned contract for GIBS and OpenTopography implementations

## Parallel Merge Strategy
1. Land E3 first if E1 or E2 need shared type names or metadata layout decisions.
2. Otherwise E1 and E2 can start immediately in parallel.
3. Rebase E1 onto E2 before final Tier 1 integration because loader behavior depends on valid-request semantics.
4. Do not start provider fetch work until all three packets agree on cache path and metadata layout.

## Tier 1 Exit Checklist
- local tile resolution supports starter-plus-cache lookup
- base Earth tile validity is no longer hard-limited by `config.tiles`
- cache schema and provider contract are written down and versioned
- no regressions to bundled starter demos
