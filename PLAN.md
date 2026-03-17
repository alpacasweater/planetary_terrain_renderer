# Project Excellence Plan

## Diagnosis: Why Terrain Looks Flat

Five compounding problems make the terrain appear smooth despite mountains being real:

### 1. Earth Terrain Config Has Wrong Height Range
`assets/terrains/earth/config.tc.ron` declares `min_height: -9907.558`, `max_height: 6147.223`.
Everest is 8,848 m. Streamed tiles for mountain peaks contain raw float values **above the
configured max**, causing the shader's `inverse_mix` and the displacement vertex math to clamp or
misrepresent those heights. Fix: widen to `-11000` / `9000`.

### 2. Base LOD Count Is Only 3
The config has `lod_count: 3`, giving LODs 0–2, each tile 128×128 px. At LOD 2 the globe face has
4×4 tiles = ~512 px across ~6,670 km → **~13 km/pixel**. Mountains narrower than 13 km are
invisible in the base mesh. Streaming promises LOD 7, but until those tiles arrive the mesh is
a smooth sphere. Base LOD must be raised and/or starter tiles must cover a wider range.

### 3. Height Streaming Is Off By Default
`minimal_globe.rs` only enables height streaming when `TERRAIN_STREAM_ONLINE=1` AND
`TERRAIN_STREAM_HEIGHT=1` are both set. No `.env` auto-loading exists. The user sees a flat globe
and has no idea why.

### 4. Default Camera Points at a Low-Elevation Area
`DEFAULT_CAMERA_ALTITUDE_M: 40_000` (40 km) is reasonable, but the default lat/lon points
somewhere with unremarkable relief. Without mountains below, there is nothing to see.

### 5. No dotenvy Auto-Loading
`streaming_warmup_globe.rs` tells users to "source `.env.opentopography.local`" but **no example
actually loads it**. Users must manually export the env var every shell session.

---

## Phase 1 — Secrets & Environment (Done / Immediate)

- [x] Create `.env.opentopography.local` with `OPENTOPOGRAPHY_API_KEY=<key>` (gitignored).
- [x] Add `.env*.local` to `.gitignore`.
- [ ] Add `dotenvy = "0.15"` to `[dev-dependencies]` in `Cargo.toml`.
- [ ] In every example's `main()`, call
  `let _ = dotenvy::from_filename_override(".env.opentopography.local");`
  **before** reading any env var. This makes the API key "just work" without shell exports.

---

## Phase 2 — Fix the Terrain Config

**File:** `assets/terrains/earth/config.tc.ron`

```diff
-    min_height: -9907.558,
-    max_height: 6147.223,
+    min_height: -11000.0,
+    max_height:  9000.0,
```

Rationale: Mariana Trench is −10,984 m; Everest is 8,848 m. A margin of ~150 m each side is
sufficient. The streamed tiles store **raw float meters** (Gray32Float / R32F), so this change
only corrects the shader range — no tile re-download is required.

---

## Phase 3 — Fix `minimal_globe` UX

This is the flagship example. It must work with zero configuration for a new user who has the API
key file, and must display visually compelling terrain immediately.

### 3a. Auto-enable Height Streaming When Key Is Present
```rust
// After dotenvy load, check for key and set stream_height default:
let api_key_present = std::env::var("OPENTOPOGRAPHY_API_KEY")
    .or_else(|_| std::env::var("OPEN_TOPOGRAPHY_API_KEY"))
    .map(|v| !v.trim().is_empty())
    .unwrap_or(false);

// Use api_key_present as the default for stream_height (overridden by flags).
```

### 3b. Default Camera to a Mountainous Area
Change defaults from wherever they currently point to:
```rust
const DEFAULT_TARGET_LAT: f64  = 27.988;   // Near Everest, Nepal
const DEFAULT_TARGET_LON: f64  = 86.925;
const DEFAULT_CAMERA_ALTITUDE_M: f32 = 25_000.0;  // 25 km — mountains clearly visible
const DEFAULT_CAMERA_BACKOFF_M: f32  = 10_000.0;
```
Himalayan terrain has the most dramatic relief on Earth, making it the best showcase.

### 3c. Print Helpful Startup Diagnostics
After parsing options, print a clear status block:
```
[minimal_globe] Terrain root:     terrains/earth
[minimal_globe] Base LODs:        3  (from config)
[minimal_globe] Streaming target: LOD 7
[minimal_globe] Imagery streaming: ENABLED  (NASA GIBS)
[minimal_globe] Height streaming:  ENABLED  (OpenTopography AW3D30)
[minimal_globe] Camera:           lat=27.99 lon=86.93 alt=25000 m  (Everest region)
```
If the API key is absent but `--stream-height` was requested, print a clear error and exit, rather
than silently falling back to a flat globe.

### 3d. Cleaner `--help` Output
Rewrite to show concrete examples:
```
USAGE:
    cargo run --example minimal_globe [OPTIONS]

OPTIONS:
    --stream-height         Enable elevation streaming via OpenTopography
    --stream-online         Enable imagery streaming via NASA GIBS
    --max-lod <N>           Maximum streamed LOD (default: 7)
    --terrain-root <PATH>   Asset path to terrain folder (default: terrains/earth)
    --target-lat <DEG>      Camera target latitude  (default: 27.99)
    --target-lon <DEG>      Camera target longitude (default: 86.93)
    --altitude <M>          Camera altitude in metres (default: 25000)

ENVIRONMENT:
    OPENTOPOGRAPHY_API_KEY  Required for --stream-height. Auto-loaded from
                            .env.opentopography.local if present.

QUICK START (with API key file already in place):
    cargo run --example minimal_globe
```

### 3e. Raise Max Inflight for Height Streaming
The current default `max_inflight_requests: 4` with `DEFAULT_HEIGHT_STREAM_MAX_INFLIGHT: 2` is
too conservative. Raise to `6` inflight total, and allow `4` for height, so the streaming
queue drains faster on first load.

---

## Phase 4 — Streaming Pipeline Audit

The `opentopography.rs` pipeline:
1. Fetches GeoTIFF from OpenTopography.
2. Decodes to `Vec<f32>` (raw metres, signed — handles negative values for bathymetry).
3. Validates range against `PLAUSIBLE_EARTH_MIN_HEIGHT_M = -20_000.0` / `MAX = 20_000.0` (correct).
4. Bilinear-resamples to tile resolution.
5. Re-encodes as `Gray32Float` TIFF → stored in cache.

The tile loader reads this back as `R32F` and uploads to GPU. The vertex shader uses the raw
metre value for radial displacement relative to `min_height` / `max_height`.

**Suspected bug — confirm and fix:** Verify that `sample_height` in the WGSL bindings returns
raw metres, not a [0,1] normalised value. If the vertex displacement shader expects [0,1] but
receives raw metres (e.g. 8000.0), the displacement would be wildly wrong and mountains would
protrude massively or be invisible. Add a debug print or test case confirming the value contract.

**Action:** Read `src/render/terrain_bind_group.rs` and the bevy_terrain WGSL `attachments.wgsl`
to trace the exact byte path from cached tile → GPU texture → shader sample → vertex position.
Confirm the scale/offset math is applied correctly.

---

## Phase 5 — Example Cleanup

| Example | Status | Action |
|---|---|---|
| `minimal_globe.rs` | Broken UX | Full rewrite per Phase 3 |
| `spherical.rs` | Redundant / less friendly | Reduce to a stub that defers to minimal_globe, or delete |
| `streaming_warmup_globe.rs` | Useful (cache warmer) | Add dotenvy, clean up env var docs |
| `precision_demo.rs` | Niche test | Leave as-is |
| `spherical_multires.rs` | Advanced demo | Leave as-is |

---

## Phase 6 — Validation Checklist

After implementing all phases, validate:

- [ ] `cargo run --example minimal_globe` with `.env.opentopography.local` present shows
  mountains in the Himalaya region within 30 seconds of startup.
- [ ] The terrain visually shows Everest (~8,848 m) protruding clearly at 25 km altitude.
- [ ] `cargo run --example minimal_globe --help` prints clear, accurate help.
- [ ] `cargo run --example minimal_globe` **without** `.env.opentopography.local` prints a
  helpful notice and runs with imagery only (no crash, no silent flat globe).
- [ ] Streaming tiles appear progressively as the camera descends — no pop-in cliff.
- [ ] Oceans appear below sea level (negative height, blue gradient) and land above (albedo).
- [ ] Height cache tiles survive a restart without re-downloading.

---

## Implementation Order

1. **Phase 1** (env / dotenvy) — unblocks everything else.
2. **Phase 2** (terrain config height range) — prevents shader clamping.
3. **Phase 3a + 3b** (auto-enable + camera) — makes the demo visually correct immediately.
4. **Phase 4** (pipeline audit) — confirms or fixes the displacement math.
5. **Phase 3c + 3d + 3e** (diagnostics, help, inflight tuning) — polish.
6. **Phase 5** (example cleanup) — final tidying.
7. **Phase 6** (validation) — sign-off.
