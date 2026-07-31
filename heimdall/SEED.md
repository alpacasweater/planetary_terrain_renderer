# heimdall seed — extraction instructions

This directory is the complete founding state of the **heimdall** project (see `README.md` and
`docs/design.md` here), parked inside `planetary_terrain_renderer` only because the session that
authored it could not be granted push access to a new repository. It is meant to be **extracted
into its own repo** (`heimdall`, private, under Swarm-Command or alpacasweater) and then deleted
from this one.

Two ways to extract:

**A. Preserve the founding commit history (preferred).** `heimdall.bundle` is a verified git
bundle of the real 3-commit history:

```sh
git clone heimdall.bundle heimdall && cd heimdall
git remote set-url origin <URL of the new empty heimdall repo>
git push -u origin main
```

**B. Fresh start from the tree.** Copy every file in this directory except `heimdall.bundle` and
`SEED.md` into the new repo root and commit.

Either way, verify with `cargo test` (7 tests; requires access to the private `small_world` repo),
then:

1. Add the `SMALL_WORLD_TOKEN` repo secret (fine-grained PAT with read access to
   `Swarm-Command/small_world`) so CI can fetch the private dependency.
2. Delete this `heimdall/` directory from `planetary_terrain_renderer`.
3. Follow `docs/roadmap.md` — next milestone is **M0** (frame proof).

Chore to schedule: merge `small_world`'s `claude/terrain-renderer-refactor-9z4mfc` branch (adds
`EGM96::from_bytes`) into small_world `main`, then re-pin the `small_world` rev in `Cargo.toml`
here and in `planetary_terrain_renderer`.
