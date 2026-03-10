# GitHub Issue Workflow

This repository uses GitHub Issues as the control plane for plan execution and pull-request review.

## Plan Mapping

The current tracked plans are:

- `Geo Coordinate API` milestone for [`docs/geo-coordinate-api-plan.md`](./geo-coordinate-api-plan.md)
- `Performance Orchestration` milestone for [`docs/agent-orchestration-plan-2026-03-07.md`](./agent-orchestration-plan-2026-03-07.md)
- `Physical Truth` milestone for [`docs/physical_truth_orchestration_plan_2026-03-07.md`](./physical_truth_orchestration_plan_2026-03-07.md)

Each plan should have:

- one tracking issue labeled `tracking`
- one issue per active task packet or implementation task labeled `task`
- a shared milestone for filtering PRs and open work

## Labels

Use a small label set:

- `tracking`: umbrella issue for a plan or milestone
- `task`: executable work item
- `blocked`: task cannot proceed because of a named dependency
- `plan:geo-api`: work from the geo coordinate API plan
- `plan:perf`: work from the performance orchestration plan
- `plan:truth`: work from the physical-truth plan

Keep the default GitHub labels for generic triage such as `enhancement`, `documentation`, or `bug`.

## Issue Structure

Tracking issues should contain:

- the source plan document
- success criteria
- the live checklist of child issues
- the current blockers or risk notes

Task issues should contain:

- the plan doc and task identifier
- the intended scope and ownership boundary
- exact acceptance criteria
- the validation commands or artifact paths needed for review

If a task changes scope beyond its plan boundary, update the tracker before the PR merges.

## Pull Requests

Every PR should:

- link one primary issue with `Closes #...` or `Relates #...`
- name the source plan doc and task ID
- include the exact validation commands that were run
- attach or link the benchmark/correctness artifacts when the issue requires them

The PR template in [`.github/pull_request_template.md`](../.github/pull_request_template.md) is the default review contract.
