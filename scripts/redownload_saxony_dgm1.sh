#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASE_SCRIPT="$ROOT_DIR/scripts/download_saxony_dgm1.sh"

OUTPUT_ROOT="${OUTPUT_ROOT:-source_data/saxony_dgm1}"
URL_LIST="${URL_LIST:-$OUTPUT_ROOT/urls.txt}"
ZIP_DIR="${ZIP_DIR:-$OUTPUT_ROOT/zip}"

WORKERS="${WORKERS:-8}"
DISCOVER_WORKERS="${DISCOVER_WORKERS:-24}"
REFRESH_URLS="${REFRESH_URLS:-1}"
REMOVE_BAD_ZIPS="${REMOVE_BAD_ZIPS:-1}"
DRY_RUN="${DRY_RUN:-0}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

usage() {
  cat <<USAGE
Usage: $(basename "$0")

This script resumes/redownloads Saxony DGM1 ZIPs while avoiding any already
downloaded and well-formed ZIP files.

Behavior:
  1) Optionally refreshes URL list from source (REFRESH_URLS=1 by default)
  2) Verifies existing ZIP files with unzip -tqq
  3) Downloads only URLs whose ZIP is missing or invalid locally

Environment overrides:
  OUTPUT_ROOT, URL_LIST, ZIP_DIR
  WORKERS
  DISCOVER_WORKERS
  REFRESH_URLS=1|0
  REMOVE_BAD_ZIPS=1|0
  DRY_RUN=1
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" || "${1:-}" == "help" ]]; then
  usage
  exit 0
fi

if [[ ! -x "$BASE_SCRIPT" ]]; then
  echo "Missing helper script: $BASE_SCRIPT" >&2
  exit 1
fi

require_cmd awk
require_cmd mktemp
require_cmd sed
require_cmd sort
require_cmd unzip
require_cmd wc

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_ROOT" "$ZIP_DIR"

if [[ "$REFRESH_URLS" == "1" || ! -s "$URL_LIST" ]]; then
  echo "[discover] Refreshing URL list from source..."
  WORKERS="$DISCOVER_WORKERS" OUTPUT_ROOT="$OUTPUT_ROOT" URL_LIST="$URL_LIST" ZIP_DIR="$ZIP_DIR" "$BASE_SCRIPT" discover
fi

if [[ ! -s "$URL_LIST" ]]; then
  echo "URL list is missing or empty: $URL_LIST" >&2
  exit 1
fi

valid_names="$(mktemp)"
need_urls="$(mktemp)"
trap 'rm -f "$valid_names" "$need_urls"' EXIT

echo "[verify] Checking existing ZIP integrity..."
bad_count=0
for zip_file in "$ZIP_DIR"/*.zip; do
  [[ -e "$zip_file" ]] || break
  if unzip -tqq "$zip_file" >/dev/null 2>&1; then
    basename "$zip_file" >> "$valid_names"
  else
    bad_count=$((bad_count + 1))
    if [[ "$REMOVE_BAD_ZIPS" == "1" ]]; then
      rm -f "$zip_file"
    fi
  fi
done
sort -u -o "$valid_names" "$valid_names"

awk '
  FILENAME == ARGV[1] { valid[$1] = 1; next }
  {
    n = split($0, parts, "/")
    name = parts[n]
    if (!(name in valid)) print $0
  }
' "$valid_names" "$URL_LIST" > "$need_urls"

total_count="$(wc -l < "$URL_LIST" | tr -d ' ')"
valid_count="$(wc -l < "$valid_names" | tr -d ' ')"
need_count="$(wc -l < "$need_urls" | tr -d ' ')"

echo "[plan] Total URLs: $total_count"
echo "[plan] Valid local ZIPs: $valid_count"
echo "[plan] Removed/flagged bad ZIPs: $bad_count"
echo "[plan] URLs to download: $need_count"

if [[ "$need_count" == "0" ]]; then
  echo "[done] Nothing to download."
  exit 0
fi

if [[ "$DRY_RUN" == "1" ]]; then
  echo "[dry-run] Sample URLs pending:"
  sed -n '1,20p' "$need_urls"
  exit 0
fi

echo "[download] Downloading missing/corrupt ZIPs..."
OUTPUT_ROOT="$OUTPUT_ROOT" URL_LIST="$need_urls" ZIP_DIR="$ZIP_DIR" WORKERS="$WORKERS" "$BASE_SCRIPT" download

echo "[done] Redownload pass finished."
