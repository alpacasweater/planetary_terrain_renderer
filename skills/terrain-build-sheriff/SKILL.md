---
name: terrain-build-sheriff
description: Repair and harden workspace build and test health for `planetary_terrain_renderer`. Use when Cargo check, test, or example builds fail, when workspace members drift on dependency versions, or when validation gates must be made reliable before optimization work proceeds.
---

# Terrain Build Sheriff

Use this skill to restore a green workspace baseline quickly and keep it green.

## Workflow
1. Establish the failing baseline first.
   Run `cargo check --workspace`, `cargo test --workspace`, and any failing package-specific command named in the task packet.
2. Find the smallest root cause.
   Prefer dependency graph inspection, version alignment, and type provenance over patching call sites blindly.
3. Fix the workspace, not just one crate.
   Shared crate version drift, feature drift, and duplicate math crates are higher priority than local workarounds.
4. Re-run the exact failing commands plus a workspace-wide validation pass.
5. Record the failure signature, root cause, and the new validation commands in the handoff.

## Rules
- Treat build health as a blocker for downstream agents.
- Prefer shared dependency alignment in workspace manifests over adapters or casts.
- Do not claim success until the original failing command and the workspace validation pass both succeed.
- If a build remains blocked by an external system dependency, document the exact missing dependency and the last passing command.

## Outputs
- Green build/test commands for the targeted workspace scope.
- Short root-cause summary.
- File list of manifest and source changes.
- Follow-up risks, if any.

Read [references/hotspots.md](references/hotspots.md) for the usual failure points and commands.
