# Saxony Workflow Review Feedback (2026-03-05)

## Reviewer Roles

The review was run as a role-based pass across these functions:

- GIS/Data Pipeline Reviewer
- Reliability/SRE Reviewer
- Rendering/Performance Reviewer
- Security/Operational Reviewer

## Critical Feedback

1. `P1` Stale handoff docs were snapshot-based and quickly became incorrect.
   - Risk: Operators resume from invalid counts and wrong process assumptions.
   - Action: Replaced with evergreen workflow doc + live `status` command.

2. `P1` Download resume path could skip corrupted ZIPs when only checking file size.
   - Risk: Corrupt archives silently propagate to extract/preprocess stages.
   - Action: Added optional `VERIFY_EXISTING_ZIPS` and `VERIFY_DOWNLOADED_ZIPS`
     with invalid ZIP logging and optional deletion.

3. `P1` No single command to report progress and disk pressure.
   - Risk: Slow incident response and poor handoff visibility.
   - Action: Added `download_saxony_dgm1.sh status`.

4. `P1` Disk usage stayed high even after successful extraction.
   - Risk: Prevents full dataset completion on constrained hosts.
   - Action: Added `PURGE_ZIPS_AFTER_EXTRACT=1` option.

5. `P2` Demo setup script did not fail fast when preprocess binary or TIFF inputs
   were missing.
   - Risk: Confusing runtime failures and wasted operator time.
   - Action: Added explicit checks and a help/usage path.

6. `P2` Some workflow docs used machine-specific absolute paths.
   - Risk: Lower reproducibility for other contributors.
   - Action: Consolidated docs and standardized workflow guidance.

## Optimization Opportunities (Next)

1. Add a remote-manifest mode using WebDAV listing metadata (size/mtime/etag) to
   reduce full-grid probe latency during discovery.
2. Persist a local download manifest (`jsonl`) with per-file checksum/size/mtime to
   avoid repeated ZIP integrity scans.
3. Add optional batched extraction verification to detect silently truncated TIFF
   outputs early.
4. Add CI shell linting (`shellcheck`) and script smoke tests (`bats`) for script
   regressions.
5. Add a bounded backoff strategy for repeated HTTP failures to reduce noisy retries
   under source throttling.

## Reliability Guardrails Recommended for Large Runs

- Use `WORKERS` values matched to network capacity (avoid extreme fan-out).
- Enable ZIP verification for long unattended runs:
  - `VERIFY_EXISTING_ZIPS=1 VERIFY_DOWNLOADED_ZIPS=1 REMOVE_BAD_ZIPS=1`
- Run status snapshots periodically:
  - `./scripts/download_saxony_dgm1.sh status`

