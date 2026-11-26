# ADR 0002 — `pin_propagate`: carry a new revision of a shared dependency to every consumer

- Status: proposed (owner's directive 2026-11-25: pins are updated automatically; first seen with the `actautoma` pin of `Actit` and `actrepo`, which blocked `actrepo clean` until three hand-made steps were done).
- Date: 2026-11-26

## Context

OpenActe repositories depend on each other by git revision (`actautoma = { git = …, rev = "…" }`). After a change merges in a shared repository, each consumer needs a pull request that bumps the `rev` and refreshes `Cargo.lock`. Done by hand on 2026-11-25: Actit pin PR #22, then the `actrepo` pin inside the `clean` branch, in this order because `actrepo` pins `actit`, which pins `actautoma`.

Today `actrepo pin <DEP> [REV]` rewrites every consumer found in the workspace file, in place, in whatever state each checkout is. It opens no branch, no PR, ignores order, and fails with "no workspace repository pins …" when no `*.code-workspace` lists the consumers.

## Decision

1. **One library function and one verb:** `pin::propagate(runner, root, dep, rev, options)` and `actautoma pin-propagate DEP [--rev REV] [--root DIR] [--dry-run]`. `REV` defaults to `origin/main` of `DEP`'s own repository.
2. **Owner of the manifest edit:** the `rev_of` / `with_rev` code of `actrepo/src/pin/manifest.rs` moves into `actautoma::pin_manifest` (this crate owns workflows; `actrepo` already depends on it). `actrepo pin` then calls it. It is moved, not copied: one slice, with `actrepo` migrated by its own PR.
3. **Consumers are found, not listed:** every direct sub-folder of `--root` (default `~/Source/Acteble/openacte`) whose `Cargo.toml` pins `DEP` by git `rev`. A consumer already on `REV` is reported `UpToDate`.
4. **Order:** a consumer is handled after every consumer it depends on (topological over the same pin graph). A cycle is an error naming its members.
5. **Per consumer, in an isolated worktree** cut from `origin/main` (never the owner's checkout): rewrite the `rev`, run `cargo update -p DEP`, commit by pathspec (`Cargo.toml`, `Cargo.lock`), push the branch `chore/bump-DEP-<short rev>`, open the PR. The gate runs through `actqueue run` with the hook wait limit (actqueue #37), so a gate that cannot get a slot fails the commit instead of holding `index.lock`.
6. **Outcomes are a closed type:** `UpToDate`, `Planned` (dry run), `Opened { pr }`, `Merged { pr }`, `Failed { step, output }`. One line each; exit non-zero on any `Failed`. A failed consumer stops the walk: its dependants would pin a revision that was never merged.
7. **Merge only where authorised.** With `--merge` it merges through the forge port, and only for repositories where merging is authorised (not `acteble_api`, `melos_apps`, nor the emvi repositories). Without it, the PR stays open and the next consumer waits.
8. **Never silent:** a consumer whose tree cannot be cut (no `origin/main`, no `Cargo.lock`) is `Failed`, with the command and its output.

## Slices (each its own PR, test first, `cargo mutants` with no survivor)

1. Discovery and order: `plan(root, dep, rev)` returns the ordered consumers and their old revisions. Pure over the file system; no git.
2. Move `pin_manifest` here; migrate `actrepo pin`. Done 2026-11-26 (actautoma #43, Actit #23, actrepo #90).
3. Per-consumer execution through `Runner`, `--dry-run` first. Done 2026-11-26 as a library (`pin_propagate`, `pin_bump`); the `actautoma pin-propagate` CLI verb comes with slice 4, so its options appear once.
4. `--merge`, then the walk across levels. Done 2026-11-26 (verb in #46, merge and levels in #47): a dependant that pins an earlier consumer is bumped to both `DEP` and that consumer's `origin/main` head in one PR, after the earlier one is merged (`Blocked` until then); emvi folders are never consumers.
5. `actrepo pin-propagate` as the thin verb, and the skill `openacte-orchestrate` updated.

## Consequences

- The 2026-11-25 chain becomes `actautoma pin-propagate actautoma`.
- `actrepo pin` keeps working while slice 2 lands; its alias stays until the last consumer is merged.
- A pin PR still waits for the gate of each consumer: the queue is the throttle, not a reason to skip it.
