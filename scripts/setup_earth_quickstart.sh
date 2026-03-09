#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${ROOT_DIR}/source_data"
TERRAIN_DIR="${ROOT_DIR}/assets/terrains/earth"
CANONICAL_HEIGHT_TIF="${SOURCE_DIR}/gebco_earth.tif"
CANONICAL_ALBEDO_TIF="${SOURCE_DIR}/true_marble.tif"
TMP_DIR="${TMP_DIR:-/tmp/terrain_earth_quickstart_tmp}"

# Public COG distribution of the GEBCO 2024 bathymetry grid from Stanford NatCap.
GEBCO_COG_URL="${GEBCO_COG_URL:-https://storage.googleapis.com/natcap-data-cache/global/gebco/gebco_bathymetry_2024_global.tif}"
EARTH_WIDTH="${EARTH_WIDTH:-8000}"
EARTH_LOD_COUNT="${EARTH_LOD_COUNT:-3}"
EARTH_TEXTURE_SIZE="${EARTH_TEXTURE_SIZE:-128}"
EARTH_BORDER_SIZE="${EARTH_BORDER_SIZE:-4}"
EARTH_MIP_LEVEL_COUNT="${EARTH_MIP_LEVEL_COUNT:-4}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

first_existing_file() {
  local candidate
  for candidate in "$@"; do
    if [[ -n "${candidate}" && -f "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

need_cmd cargo

mkdir -p "${SOURCE_DIR}" "${ROOT_DIR}/assets/terrains"

HEIGHT_SOURCE_TIF="$(first_existing_file "${HEIGHT_SOURCE_TIF:-}" "${CANONICAL_HEIGHT_TIF}" || true)"
ALBEDO_SOURCE_TIF="$(first_existing_file "${ALBEDO_SOURCE_TIF:-}" "${CANONICAL_ALBEDO_TIF}" || true)"

if [[ -z "${HEIGHT_SOURCE_TIF}" ]]; then
  need_cmd gdal_translate
  HEIGHT_SOURCE_TIF="${CANONICAL_HEIGHT_TIF}"
  echo "[1/3] Creating low-res Earth height source raster at ${HEIGHT_SOURCE_TIF}"
  GDAL_DISABLE_READDIR_ON_OPEN=EMPTY_DIR \
  CPL_VSIL_CURL_ALLOWED_EXTENSIONS=.tif \
  gdal_translate \
    -of GTiff \
    -outsize "${EARTH_WIDTH}" 0 \
    -co TILED=YES \
    -co COMPRESS=DEFLATE \
    -co PREDICTOR=2 \
    -co BIGTIFF=IF_SAFER \
    "/vsicurl/${GEBCO_COG_URL}" \
    "${HEIGHT_SOURCE_TIF}"
else
  echo "[1/3] Reusing Earth height source raster at ${HEIGHT_SOURCE_TIF}"
fi

echo "[2/3] Rebuilding starter Earth terrain at ${TERRAIN_DIR}"
rm -rf "${TERRAIN_DIR}"
cargo run -p bevy_terrain_preprocess -- \
  "${HEIGHT_SOURCE_TIF}" \
  "${TERRAIN_DIR}" \
  --temp-path "${TMP_DIR}" \
  --overwrite \
  --no-data source \
  --data-type float32 \
  --fill-radius 0 \
  --lod-count "${EARTH_LOD_COUNT}" \
  --attachment-label height \
  --ts "${EARTH_TEXTURE_SIZE}" \
  --bs "${EARTH_BORDER_SIZE}" \
  --m "${EARTH_MIP_LEVEL_COUNT}" \
  --format r32f

if [[ -n "${ALBEDO_SOURCE_TIF}" ]]; then
  echo "[3/3] Adding Earth albedo from ${ALBEDO_SOURCE_TIF}"
  cargo run -p bevy_terrain_preprocess -- \
    "${ALBEDO_SOURCE_TIF}" \
    "${TERRAIN_DIR}" \
    --temp-path "${TMP_DIR}" \
    --overwrite \
    --no-data source \
    --data-type source \
    --fill-radius 0 \
    --lod-count "${EARTH_LOD_COUNT}" \
    --attachment-label albedo \
    --ts "${EARTH_TEXTURE_SIZE}" \
    --bs "${EARTH_BORDER_SIZE}" \
    --m "${EARTH_MIP_LEVEL_COUNT}" \
    --format rg8u
else
  echo "[3/3] No local Earth albedo raster found; leaving the starter dataset height-only."
fi

echo
echo "Starter Earth dataset is ready."
echo "Run: cargo run --example spherical"
