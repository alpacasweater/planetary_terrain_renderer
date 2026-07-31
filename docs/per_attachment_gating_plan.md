# Per-Attachment Gating (defect 2) — implementation plan

Status: **not implemented**. This is the one streaming defect whose failure mode is silent GPU
texel corruption, and it cannot be visually validated in a headless environment. The plan below
is written so a GPU-capable session can execute and verify it quickly.

## The defect

A tile leaves `pending_stream_tiles` only when **every** attachment (height **and** albedo) is
present locally (`TileAtlas::missing_local_attachments` must be empty). Height streams first by
priority, but its finer geometry stays invisible until the imagery for the same tile also lands,
so nothing refines during warm-up and detail pops in bulk. The renderer still shows the coarse
ancestor meanwhile (correct, just unrefined).

Why it can't be a naive "promote on height alone": each attachment is a separate GPU texture
array indexed by `atlas_index`. Promoting a tile whose albedo texels were never uploaded leaves
that slot's albedo showing the **previous occupant's** data — visible corruption.

## Recommended approach: derived stand-in albedo (GPU-safe)

Keep the "all attachments present" gate — do **not** touch the GPU upload path or shaders.
Instead, satisfy a missing non-height attachment with a coarse stand-in derived from the
best-loaded ancestor, exactly as height already does
(`scheduler::materialize_derived_height_into_cache` / `derive_child_height_tile`).

1. Add an RGB analogue of `derive_child_height_tile` (bilinear upsample of an ancestor albedo
   tile into the child's sub-region) and `materialize_derived_albedo_into_cache`, reusing
   `streaming/util.rs` helpers and `load_best_local_*_ancestor`.
2. When a tile has height present but albedo still streaming, derive the stand-in albedo from the
   ancestor and write it to cache. The tile now has both attachments → it promotes with fine
   geometry + coarse (upsampled) albedo. No shader/GPU change; no corruption risk.
3. When the true fine albedo streams in, it overwrites the cache tile and the existing
   completion-event reload (`StreamingCompletionEvents` → `try_promote_pending_tile`, and the
   upload path) refreshes the slot to the real texels.
4. Cache the derived stand-in under a distinct source id (`local/derived_albedo_from_*`) so it is
   visibly a placeholder and is always superseded by a real fetch.

Validation: run the `minimal_globe` example over Everest; confirm geometry sharpens ahead of
imagery during warm-up and that imagery then resolves to full detail with no seams or stale
colour.

## Alternative: neutral-fallback upload (needs GPU work)

Promote on height alone and upload a neutral texture to the missing attachment's slot, then
re-upload when the real attachment arrives. Lower-latency but it edits the residency/upload path
(`gpu_tile_atlas`) and the failure mode is corruption — only worth it if the derived approach is
too coarse in practice. If taken, gate promotion on height, track per-tile which attachments are
fallbacks, and drive the re-upload from `StreamingCompletionEvents`.

## Guardrails

Whichever path: the ancestor-fallback invariant (invariant 4 in `docs/fork_review_2026-07-30.md`
§2) must hold — missing data always degrades to a coarser tile, never a hole or stale slot. Add a
state-machine test that a height-present/albedo-missing tile promotes only once a (real or
derived) albedo exists, and reloads when the real albedo lands.
