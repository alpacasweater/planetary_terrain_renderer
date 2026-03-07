---
name: terrain-release-verifier
description: Validate that `planetary_terrain_renderer` is ready to merge by checking build health, correctness metrics, benchmark regressions, and visual evidence. Use after feature or optimization branches land and before merge decisions are made.
---

# Terrain Release Verifier

Use this skill to produce a merge-ready go or no-go decision backed by reproducible evidence.

## Workflow
1. Validate workspace build and test health.
2. Validate correctness metrics and residual reporting.
3. Validate benchmark and visual evidence.
4. Compare results against the current gates.
5. Produce a short go or no-go summary with exact blockers.

## Gates
- workspace build and test commands green
- correctness metrics rerunnable and reported with mean abs, p95 abs, max abs, and RMS
- benchmark captures nonblank
- benchmark p95 and p99 are not worse than the accepted baseline unless the change explicitly trades speed for correctness and that trade is documented

## Rules
- Do not waive failures without writing down the reason and the owner.
- Use the same commands and datasets the feature agent used whenever possible.
- Keep the final summary short and binary.

## Outputs
- Validation command list.
- Gate table with pass or fail.
- Go or no-go summary.

Read [references/hotspots.md](references/hotspots.md) before running the final validation pass.
