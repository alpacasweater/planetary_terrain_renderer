# Saxony DGM1 Workflow

This document covers the download, resume, redownload, extraction, and demo flow
for the Saxony DGM1 dataset.

## Scripts

- `scripts/download_saxony_dgm1.sh`
  - Discovers available tiles, downloads ZIPs, extracts TIFFs, and reports status.
- `scripts/redownload_saxony_dgm1.sh`
  - Refreshes the source URL list, validates local ZIPs, and downloads only
    missing/corrupt ZIPs.
- `scripts/setup_saxony_partial_demo.sh`
  - Builds the `saxony_partial` terrain overlay from extracted TIFFs.

## Quick Start

```bash
cd /path/to/planetary_terrain_renderer

# 1) discover available URLs
./scripts/download_saxony_dgm1.sh discover

# 2) download ZIPs
WORKERS=12 ./scripts/download_saxony_dgm1.sh download

# 3) extract TIFFs
WORKERS=12 ./scripts/download_saxony_dgm1.sh extract

# 4) build overlay + run demo
./scripts/setup_saxony_partial_demo.sh
MULTIRES_OVERLAYS=saxony cargo run --example spherical_multires
```

## Status / Handoff

```bash
./scripts/download_saxony_dgm1.sh status
```

This prints URL, ZIP, and TIFF counts, plus ZIP/extracted sizes and free space.

## Resume / Repair

If the download was interrupted or files are suspected to be corrupt:

```bash
WORKERS=8 ./scripts/redownload_saxony_dgm1.sh
```

Useful switches:

```bash
# preview what would be downloaded
DRY_RUN=1 REFRESH_URLS=0 ./scripts/redownload_saxony_dgm1.sh

# keep invalid ZIPs for manual inspection
REMOVE_BAD_ZIPS=0 ./scripts/redownload_saxony_dgm1.sh
```

## Disk-Space Optimization

After extraction, ZIPs are optional for running the demo. You can reclaim space:

```bash
PURGE_ZIPS_AFTER_EXTRACT=1 WORKERS=12 ./scripts/download_saxony_dgm1.sh extract
```

If extraction is already complete, removing `source_data/saxony_dgm1/zip/*.zip`
is safe for demo/runtime use.

## Performance Tuning

- Increase `WORKERS` to match available bandwidth and CPU.
- Discovery can be sped up with:
  - `DISCOVER_CONNECT_TIMEOUT`
  - `DISCOVER_MAX_TIME`
  - `DISCOVER_RETRY_MAX_TIME`
- For fastest resume runs (lower CPU validation overhead), keep:
  - `VERIFY_EXISTING_ZIPS=0`
  - `VERIFY_DOWNLOADED_ZIPS=0`

## Reliability Tuning

For stricter integrity checks:

```bash
VERIFY_EXISTING_ZIPS=1 VERIFY_DOWNLOADED_ZIPS=1 REMOVE_BAD_ZIPS=1 \
  WORKERS=8 ./scripts/download_saxony_dgm1.sh download
```

Invalid ZIPs are logged to:

- `source_data/saxony_dgm1/invalid_zips.txt`
