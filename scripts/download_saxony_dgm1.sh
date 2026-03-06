#!/usr/bin/env bash
set -euo pipefail

SHARE_TOKEN="${SHARE_TOKEN:-JCcXyifaNdLDnxZ}"
BASE_URL="${BASE_URL:-https://geocloud.landesvermessung.sachsen.de/public.php/dav/files/${SHARE_TOKEN}}"

EAST_MIN="${EAST_MIN:-33278}"
EAST_MAX="${EAST_MAX:-33502}"
EAST_STEP="${EAST_STEP:-2}"
NORTH_MIN="${NORTH_MIN:-5560}"
NORTH_MAX="${NORTH_MAX:-5720}"
NORTH_STEP="${NORTH_STEP:-2}"

WORKERS="${WORKERS:-8}"
SKIP_DISCOVERY="${SKIP_DISCOVERY:-0}"
DISCOVER_CONNECT_TIMEOUT="${DISCOVER_CONNECT_TIMEOUT:-5}"
DISCOVER_MAX_TIME="${DISCOVER_MAX_TIME:-15}"
DISCOVER_RETRY_MAX_TIME="${DISCOVER_RETRY_MAX_TIME:-60}"

OUTPUT_ROOT="${OUTPUT_ROOT:-source_data/saxony_dgm1}"
URL_LIST="${URL_LIST:-${OUTPUT_ROOT}/urls.txt}"
ZIP_DIR="${ZIP_DIR:-${OUTPUT_ROOT}/zip}"
EXTRACT_DIR="${EXTRACT_DIR:-${OUTPUT_ROOT}/extracted}"

MODE="${1:-all}"
VERIFY_EXISTING_ZIPS="${VERIFY_EXISTING_ZIPS:-0}"
VERIFY_DOWNLOADED_ZIPS="${VERIFY_DOWNLOADED_ZIPS:-0}"
REMOVE_BAD_ZIPS="${REMOVE_BAD_ZIPS:-0}"
PURGE_ZIPS_AFTER_EXTRACT="${PURGE_ZIPS_AFTER_EXTRACT:-0}"
INVALID_ZIP_LIST="${INVALID_ZIP_LIST:-${OUTPUT_ROOT}/invalid_zips.txt}"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [discover|download|extract|status|all]

Modes:
  discover   Scan a coordinate grid and write valid tile URLs to URL_LIST.
  download   Download all URLs from URL_LIST into ZIP_DIR.
  extract    Unzip all ZIP_DIR/*.zip into EXTRACT_DIR.
  status     Print current URL/ZIP/TIFF counts and disk usage.
  all        discover -> download -> extract (default).

Environment overrides:
  SHARE_TOKEN, BASE_URL
  EAST_MIN, EAST_MAX, EAST_STEP
  NORTH_MIN, NORTH_MAX, NORTH_STEP
  WORKERS
  DISCOVER_CONNECT_TIMEOUT, DISCOVER_MAX_TIME, DISCOVER_RETRY_MAX_TIME
  OUTPUT_ROOT, URL_LIST, ZIP_DIR, EXTRACT_DIR
  VERIFY_EXISTING_ZIPS=1     Validate existing ZIPs before skipping in download mode.
  VERIFY_DOWNLOADED_ZIPS=1   Validate each ZIP after download and log invalid ones.
  REMOVE_BAD_ZIPS=1          Delete invalid ZIPs when verification is enabled.
  PURGE_ZIPS_AFTER_EXTRACT=1 Delete ZIPs after successful extraction to save disk.
  INVALID_ZIP_LIST           Path for invalid ZIP report (default: OUTPUT_ROOT/invalid_zips.txt).
  SKIP_DISCOVERY=1    Skip discovery in "all" mode when URL_LIST already exists.

Example:
  WORKERS=12 ./scripts/download_saxony_dgm1.sh all
USAGE
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

discover_urls() {
  mkdir -p "$(dirname "$URL_LIST")"

  local candidates
  local found
  candidates="$(mktemp)"
  found="$(mktemp)"

  echo "[discover] Building candidate URL grid..."
  local e
  local n
  for ((e = EAST_MIN; e <= EAST_MAX; e += EAST_STEP)); do
    for ((n = NORTH_MIN; n <= NORTH_MAX; n += NORTH_STEP)); do
      printf '%s/dgm1_%d_%d_2_sn_tiff.zip\n' "$BASE_URL" "$e" "$n"
    done
  done > "$candidates"

  echo "[discover] Probing candidates with $WORKERS workers..."
  xargs -P "$WORKERS" -I '{}' bash -c '
    url="$1"
    connect_timeout="$2"
    max_time="$3"
    retry_max_time="$4"
    # Use a 1-byte range request to avoid downloading full ZIPs while still
    # working with hosts that reject HEAD requests.
    code=$(curl -sS -L -r 0-0 --connect-timeout "$connect_timeout" --max-time "$max_time" -o /dev/null -w "%{http_code}" "$url" 2>/dev/null)
    rc=$?

    # Retry once with a longer timeout if the probe failed at transport level.
    if [ "$rc" -ne 0 ] && [ "$retry_max_time" -gt "$max_time" ]; then
      code=$(curl -sS -L -r 0-0 --connect-timeout "$connect_timeout" --max-time "$retry_max_time" -o /dev/null -w "%{http_code}" "$url" 2>/dev/null)
      rc=$?
    fi

    if [ "$code" = "200" ] || [ "$code" = "206" ]; then
      printf "%s\\n" "$url"
    fi
  ' _ '{}' "$DISCOVER_CONNECT_TIMEOUT" "$DISCOVER_MAX_TIME" "$DISCOVER_RETRY_MAX_TIME" < "$candidates" | sort -u > "$found"

  mv "$found" "$URL_LIST"
  rm -f "$candidates"

  echo "[discover] Wrote $(wc -l < "$URL_LIST" | tr -d ' ') URLs to $URL_LIST"
}

download_urls() {
  if [[ ! -s "$URL_LIST" ]]; then
    echo "URL list is missing or empty: $URL_LIST" >&2
    echo "Run: $(basename "$0") discover" >&2
    exit 1
  fi

  mkdir -p "$ZIP_DIR"
  mkdir -p "$(dirname "$INVALID_ZIP_LIST")"
  : > "$INVALID_ZIP_LIST"
  echo "[download] Downloading into $ZIP_DIR with $WORKERS workers..."

  xargs -P "$WORKERS" -I '{}' bash -c '
    url="$1"
    out_dir="$2"
    verify_existing="$3"
    verify_downloaded="$4"
    remove_bad="$5"
    invalid_list="$6"
    file="$out_dir/$(basename "$url")"

    if [[ -s "$file" ]]; then
      if [[ "$verify_existing" != "1" ]]; then
        exit 0
      fi

      if unzip -tqq "$file" >/dev/null 2>&1; then
        exit 0
      fi

      printf "%s\n" "$file" >> "$invalid_list"
      if [[ "$remove_bad" == "1" ]]; then
        rm -f "$file"
      fi
    fi

    curl -sS -L --retry 6 --retry-delay 2 --retry-all-errors -C - -o "$file" "$url"

    if [[ "$verify_downloaded" == "1" ]]; then
      if ! unzip -tqq "$file" >/dev/null 2>&1; then
        printf "%s\n" "$file" >> "$invalid_list"
        if [[ "$remove_bad" == "1" ]]; then
          rm -f "$file"
        fi
      fi
    fi
  ' _ '{}' "$ZIP_DIR" "$VERIFY_EXISTING_ZIPS" "$VERIFY_DOWNLOADED_ZIPS" "$REMOVE_BAD_ZIPS" "$INVALID_ZIP_LIST" < "$URL_LIST"

  echo "[download] Completed. ZIP files: $(find "$ZIP_DIR" -maxdepth 1 -type f -name '*.zip' | wc -l | tr -d ' ')"
  if [[ -s "$INVALID_ZIP_LIST" ]]; then
    echo "[download] Invalid ZIP entries: $(wc -l < "$INVALID_ZIP_LIST" | tr -d ' ') -> $INVALID_ZIP_LIST"
  fi
}

extract_zips() {
  mkdir -p "$EXTRACT_DIR"

  local zip_list
  zip_list="$(mktemp)"
  find "$ZIP_DIR" -maxdepth 1 -type f -name '*.zip' | sort > "$zip_list"

  if [[ ! -s "$zip_list" ]]; then
    rm -f "$zip_list"
    echo "No ZIP files found in $ZIP_DIR" >&2
    exit 1
  fi

  echo "[extract] Extracting into $EXTRACT_DIR with $WORKERS workers..."
  xargs -P "$WORKERS" -I '{}' bash -c '
    zip_file="$1"
    out_dir="$2"
    purge_zip="$3"
    unzip -n -q "$zip_file" -d "$out_dir"
    if [[ "$purge_zip" == "1" ]]; then
      rm -f "$zip_file"
    fi
  ' _ '{}' "$EXTRACT_DIR" "$PURGE_ZIPS_AFTER_EXTRACT" < "$zip_list"

  rm -f "$zip_list"
  echo "[extract] Completed. GeoTIFF files: $(find "$EXTRACT_DIR" -maxdepth 1 -type f -name '*.tif' | wc -l | tr -d ' ')"
}

print_status() {
  local url_count
  local zip_count
  local tif_count

  url_count=0
  zip_count=0
  tif_count=0

  if [[ -s "$URL_LIST" ]]; then
    url_count="$(wc -l < "$URL_LIST" | tr -d ' ')"
  fi
  if [[ -d "$ZIP_DIR" ]]; then
    zip_count="$(find "$ZIP_DIR" -maxdepth 1 -type f -name '*.zip' | wc -l | tr -d ' ')"
  fi
  if [[ -d "$EXTRACT_DIR" ]]; then
    tif_count="$(find "$EXTRACT_DIR" -maxdepth 1 -type f -name '*.tif' | wc -l | tr -d ' ')"
  fi

  echo "[status] URL list: $URL_LIST"
  echo "[status] Total discovered URLs: $url_count"
  echo "[status] ZIPs present: $zip_count"
  echo "[status] Extracted TIFFs: $tif_count"
  if [[ "$url_count" -gt 0 ]]; then
    echo "[status] Remaining (URLs - ZIPs): $((url_count - zip_count))"
  fi
  if [[ -d "$ZIP_DIR" ]]; then
    echo "[status] ZIP dir size: $(du -sh "$ZIP_DIR" | awk '{print $1}')"
  fi
  if [[ -d "$EXTRACT_DIR" ]]; then
    echo "[status] Extracted dir size: $(du -sh "$EXTRACT_DIR" | awk '{print $1}')"
  fi
  echo "[status] Free space: $(df -h . | awk 'NR==2 {print $4}')"
}

main() {
  require_cmd bash
  require_cmd curl
  require_cmd df
  require_cmd du
  require_cmd find
  require_cmd mktemp
  require_cmd rm
  require_cmd sh
  require_cmd tr
  require_cmd wc
  require_cmd xargs
  require_cmd sort
  require_cmd unzip

  case "$MODE" in
    discover)
      discover_urls
      ;;
    download)
      download_urls
      ;;
    extract)
      extract_zips
      ;;
    status)
      print_status
      ;;
    all)
      if [[ "$SKIP_DISCOVERY" == "1" && -s "$URL_LIST" ]]; then
        echo "[discover] Skipping discovery (SKIP_DISCOVERY=1 and URL list exists)."
      else
        discover_urls
      fi
      download_urls
      extract_zips
      ;;
    -h|--help|help)
      usage
      ;;
    *)
      echo "Unknown mode: $MODE" >&2
      usage
      exit 1
      ;;
  esac
}

main "$@"
