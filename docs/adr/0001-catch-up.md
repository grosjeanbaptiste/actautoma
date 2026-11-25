# ADR 0001 — `catch_up`: bring a diverged branch onto its remote in one command

- Status: proposed (owner's directive 2026-11-24: actautoma should offer an automated workflow that settles this situation in one command; the design below is not yet confirmed).
- Date: 2026-11-24

## Context

A local `main` holds commits that were never pushed while `origin/main` moved on, often because another session shipped in between. Seen in `actconf` on 2026-11-24: one local commit, two remote commits, a release tag already taken (`v0.3.0`). Settling it by hand takes fetch, a look at what overlaps, a copy of the original, a rebase, conflict resolution, a gate run and a version bump. `ROADMAP.md` already plans `sync-all`, which needs a per-branch verb for the diverged case. This is that verb.

## Decision

1. **One library function and one verb:** `branches::catch_up(runner, dir, base)` and `actautoma catch-up [--base main] [--dir D]`. Same hexagonal style as `classify`: it goes through the `Runner` port, never `git` directly.
2. **Steps, in order:**
   1. refuse a dirty working tree, naming the files;
   2. `git fetch origin`;
   3. count commits ahead and behind `origin/<base>`;
   4. behind 0: nothing to catch up (report how many are ahead, to push);
   5. ahead 0: fast-forward only;
   6. diverged: **copy the original first** as the local branch `backup/<branch>/<tip>` (owner's rule: always copy the original before a merge), then dry-run with `git merge-tree`; on conflict stop and name the files, the branch untouched; otherwise `git rebase origin/<base>`.
3. **Outcomes are a closed type:** `UpToDate { ahead }`, `FastForwarded { commits }`, `Replayed { commits, backup }`, `Conflict { files, backup }`, `Dirty { files }`. One report line each; the CLI exits non-zero on `Conflict` and `Dirty`.
4. **Never outward.** `catch_up` pushes nothing, tags nothing, bumps no version. Publishing stays a separate, authorised act (`ship`).
5. **Not in this ADR:** the gate run and the version proposal (a later slice, once `sync-all` exists), and any change on `Glab`.

## Consequences

- The `actconf` case becomes one command, with the original kept in `backup/main/<tip>`.
- `sync-all` can call it for every diverged branch.
- A rebase rewrites local commits only; the backup branch makes that reversible.
- Rewriting history is acceptable here only because the commits are unpushed: when `ahead` commits already exist on `origin` under another hash, the dry run is the guard, and the verb stops at `Conflict`.
