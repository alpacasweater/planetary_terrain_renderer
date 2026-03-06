#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DATA_ROOT="${DATA_ROOT:-source_data/saxony_dgm1}"
INVALID_LIST="${INVALID_LIST:-$DATA_ROOT/invalid_zips.txt}"
EXTRACTED_DIR="${EXTRACTED_DIR:-$DATA_ROOT/extracted}"
OUTPUT_TERRAIN_DIR="${OUTPUT_TERRAIN_DIR:-assets/terrains/saxony_partial}"
TEMP_PATH="${TEMP_PATH:-/tmp/saxony_partial_tmp}"
PREPROCESS_BIN="${PREPROCESS_BIN:-./target/debug/bevy_terrain_preprocess}"

usage() {
  cat <<USAGE
Usage: $(basename "$0")

Builds the Saxony partial overlay from extracted DGM1 TIFF files.

Environment overrides:
  DATA_ROOT
  INVALID_LIST
  EXTRACTED_DIR
  OUTPUT_TERRAIN_DIR
  TEMP_PATH
  PREPROCESS_BIN
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" || "${1:-}" == "help" ]]; then
  usage
  exit 0
fi

if [[ ! -d "$EXTRACTED_DIR" ]]; then
  echo "Missing extracted directory: $EXTRACTED_DIR" >&2
  exit 1
fi

if [[ ! -x "$PREPROCESS_BIN" ]]; then
  echo "Missing preprocess binary at: $PREPROCESS_BIN" >&2
  echo "Build it with: cargo build -p bevy_terrain_preprocess" >&2
  exit 1
fi

tif_count="$(find "$EXTRACTED_DIR" -maxdepth 1 -type f -name '*.tif' | wc -l | tr -d ' ')"
if [[ "$tif_count" == "0" ]]; then
  echo "No TIFF files found in $EXTRACTED_DIR" >&2
  exit 1
fi

if [[ -s "$INVALID_LIST" ]]; then
  echo "[cleanup] Removing bad ZIPs listed in $INVALID_LIST"
  while IFS= read -r zip_path; do
    [[ -n "$zip_path" ]] || continue
    rm -f "$zip_path"
  done < "$INVALID_LIST"
fi

: > "$INVALID_LIST"

echo "[preprocess] Building saxony_partial terrain overlay..."
"$PREPROCESS_BIN" \
  "$EXTRACTED_DIR" \
  "$OUTPUT_TERRAIN_DIR" \
  --temp-path "$TEMP_PATH" \
  --overwrite \
  --no-data source \
  --data-type float32 \
  --fill-radius 0 \
  --create-mask \
  --lod-count 6 \
  --attachment-label height \
  --ts 512 \
  --bs 4 \
  --m 4 \
  --format r32f

echo
echo "[done] Saxony partial overlay is ready at $OUTPUT_TERRAIN_DIR"
echo "[done] Source TIFF count: $tif_count"
echo "Run demo with:"
echo "  MULTIRES_OVERLAYS=saxony cargo run --example spherical_multires"
