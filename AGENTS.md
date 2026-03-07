## Skills
Local skills for `planetary_terrain_renderer` live in `skills/`.
Use them when the task clearly matches the description or when a task packet in `docs/agent-orchestration-plan-2026-03-07.md` or `docs/physical_truth_orchestration_plan_2026-03-07.md` names a skill.

### Available skills
- `terrain-build-sheriff`: Repair workspace build/test failures, dependency drift, and validation gates. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-build-sheriff/SKILL.md)
- `terrain-correctness-metrics`: Measure renderer-vs-`small_world` spatial correctness and add regression metrics. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-correctness-metrics/SKILL.md)
- `terrain-geodesy-truth`: Validate renderer WGS84 transforms, axis conventions, and ellipsoid semantics against `small_world`. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-geodesy-truth/SKILL.md)
- `terrain-raster-truth`: Audit preprocess/runtime raster sample parity and projection semantics for physical-truth work. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-raster-truth/SKILL.md)
- `terrain-end-to-end-truth`: Validate end-to-end robot/drone/data placement against the rendered world. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-end-to-end-truth/SKILL.md)
- `terrain-benchmark-profiler`: Run reproducible benchmarks, captures, and CPU/GPU profiling for the renderer. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-benchmark-profiler/SKILL.md)
- `terrain-streaming-optimizer`: Optimize tile scheduling, loading, atlas residency, and upload burst behavior. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-streaming-optimizer/SKILL.md)
- `terrain-render-path-optimizer`: Optimize bind groups, buffer updates, depth resources, and render-pass overhead. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-render-path-optimizer/SKILL.md)
- `terrain-release-verifier`: Validate merge readiness against build, correctness, benchmark, and visual gates. (file: /Users/biggsba1/Documents/Playground/planetary_terrain_renderer/skills/terrain-release-verifier/SKILL.md)

### Usage rules
- Read only the named skill(s) needed for the active task.
- Prefer the skill workflow over ad hoc exploration.
- If multiple skills apply, use the minimum set and keep one skill responsible for each work packet.
- When a task packet in `docs/agent-orchestration-plan-2026-03-07.md` or `docs/physical_truth_orchestration_plan_2026-03-07.md` names a skill, treat that as the default specialization for the agent working that packet.
