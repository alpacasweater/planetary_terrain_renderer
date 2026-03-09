# Earth Streaming Cache Contract (2026-03-09)

This document defines the shared cache and provider contract for Tier 1 packet `E3`.

It is intentionally provider-agnostic.
It does not define any HTTP behavior.
It exists so cache lookup, procedural tile validity, and future remote fetchers can agree on one durable local format.

## Purpose

The renderer should remain local-first:
- bundled starter Earth provides immediate offline rendering
- streamed providers only materialize local cache tiles
- runtime loading continues to consume local terrain attachment tiles

This means every remote backend must eventually produce the same local artifact shape:
- a terrain attachment tile file
- a sidecar metadata record describing provenance and freshness

## Contract Summary

Code lives in:
- [src/streaming/cache_manifest.rs](/Users/biggsba1/Documents/rust_playground/planetary_terrain_renderer/src/streaming/cache_manifest.rs)
- [src/streaming/source_contract.rs](/Users/biggsba1/Documents/rust_playground/planetary_terrain_renderer/src/streaming/source_contract.rs)

Core types:
- `StreamingCacheManifest`
- `RegisteredStreamingSource`
- `StreamingSourceDescriptor`
- `CachedTileMetadata`
- `StreamingTileRequest`
- `StreamingTileProvider`
- `MaterializedStreamingTile`

## Cache Files

Tier 1 deliberately avoids locking in the final writable cache root.
Instead, it standardizes filenames and sidecar semantics.

Per-terrain manifest file:
- `streaming_cache_manifest.ron`

Per-tile metadata sidecar:
- same tile path with `.tile-cache.ron` extension

Example:
- tile: `.../albedo/3/0_0/3_0_2_5.tif`
- metadata: `.../albedo/3/0_0/3_0_2_5.tile-cache.ron`

## Manifest Schema

`StreamingCacheManifest` stores:
- `format_version`
- `terrain_path`
- `sources`

Each `RegisteredStreamingSource` stores:
- `descriptor`
- `freshness_policy`

Each `StreamingSourceDescriptor` stores:
- `source_id`
- `source_kind`
- `attachment_kind`

The manifest is intended to answer:
- which providers are allowed to populate this cache
- which attachment class they serve
- what freshness policy applies to their cached data

## Tile Metadata Schema

`CachedTileMetadata` stores:
- `format_version`
- `terrain_path`
- `attachment_label`
- `coordinate`
- `source`
- `fetched_at_unix_ms`
- `expires_at_unix_ms`
- `source_zoom`
- `source_revision`
- `source_content_hash`
- `source_crs`
- `encoding`

This is enough to invalidate cache entries when:
- the cache schema version changes
- the provider identity changes
- the provider says the tile is expired
- the local freshness policy rejects the tile due to age

## Freshness And Invalidation

Tier 1 standardizes three invalidation mechanisms:

1. Schema version
- if `format_version` differs from `CURRENT_STREAMING_CACHE_FORMAT_VERSION`, the tile is stale

2. Provider identity
- if the cached `source` does not match the active `StreamingSourceDescriptor`, the tile is stale

3. Age or explicit expiry
- `CacheFreshnessPolicy.max_age_seconds`
- `CachedTileMetadata.expires_at_unix_ms`

This gives enough structure for Tier 2 without prematurely baking in a network cache validator protocol.

## Provider Contract

Future remote backends implement `StreamingTileProvider`.

Minimum required behavior:
1. describe themselves with `descriptor()`
2. answer whether a request is available via `availability(...)`
3. materialize a local cache tile via `materialize_tile(...)`

The provider does not directly define render behavior.
It only transforms a `StreamingTileRequest` into a `MaterializedStreamingTile`.

`MaterializedStreamingTile` contains:
- `bytes`
- `metadata`

That keeps the provider side generic and lets the eventual cache writer own final filesystem placement.

## Deliberate Non-Decisions In Tier 1

This contract does not yet decide:
- the final writable cache root
- the asset-server integration strategy for non-`assets/` cache directories
- the scheduling model for online requests
- provider-specific authentication or retry behavior
- whether cache writing is synchronous or async

Those belong to `E1` and Tier 2.

## Intended Tier 2 Providers

The initial source stack this contract is meant to support:
- imagery: NASA GIBS
- imagery expansion: Sentinel-2 COG or similar STAC-backed sources
- height: OpenTopography-backed DEM fetches

## Acceptance Use

Tier 1 is considered complete for `E3` when:
- the manifest schema is versioned
- tile metadata is versioned
- freshness rules are explicit
- provider identity is recorded
- both imagery and height providers can target the same contract
