# Roadmap

## Directive (owner, 2026-11-23): `actrepo` and `actautoma` handle every git and gh workflow

Every workflow an agent or the owner runs with `git` or `gh` goes through
`actrepo` or `actautoma`; raw `git` / `gh` stays only for what they lack, and
each such use names the missing verb so it can be added here.

Proposed split (mine, to confirm with the owner): `actrepo` holds the single
verbs on a repository: commit, push, pull, branches, worktrees and pull requests; `actautoma`
holds the workflows that chain several of them (ship, merge a stack, report a
problem upstream). Both stay thin over `git` and `gh`, with the strictness of
the rest of the doctrine (no silent failure, a refusal names what and why).

### `pin-propagate` (ADR 0002, owner's directive 2026-11-25)

Slice 1 done (`pin_plan::plan`: consumers and bump order). Slice 2 done (`pin_manifest::with_rev` in actautoma #43; `actrepo pin` uses it, local copy removed, actrepo #90). Slice 3 done (`pin_propagate::propagate`, library). Slice 4 done (verb `actautoma pin-propagate DEP [--rev REV] [--root DIR] [--dry-run] [--merge] [--gate "CMD"]` in #46; `--merge` and the multi-level walk in #47). Slice 5 done (`actrepo pin-propagate` in actrepo #92, skill `openacte-orchestrate` in dotfiles #75). Tag pins are read since `feat/pin-tag`. Split done (2026-11-27): the library stays in `src/pin_propagate.rs`, the command line moved to `src/pin_propagate_cli.rs`, re-exported from `pin_propagate` so no caller changed. CI wait done (2026-11-27): with `--merge`, `ship` waits for a run still going (`src/ship_ci.rs`, `--ci-wait SECONDS`, default 900, `0` never waits) before deciding; a run still going after the wait holds the merge and says so. The `ship` verb itself does not wait yet. Checkout advance done (2026-11-27): after a merged bump, the consumer's own checkout is fast-forwarded to `origin/main` when it is on `main` with no tracked changes (`src/pin_advance.rs`); otherwise it is left alone and the line reads `merged URL; local checkout not advanced: <why>` (`Outcome::MergedStale`). No debt of ADR 0002 is open. The `ship` verb does not wait for CI and does not advance a checkout; each would be its own test-first PR.

### Verbs missing today (found by shipping 30 PRs on 2026-11-22/23 with raw commands)

`actrepo` has `pr create`, `pr merge`, `push` (a repository and its nested ones), `worktree list|prune`, `status`; it has no `commit` and no `pull`.
Used by hand instead:

| Raw command | Wanted verb | Owner |
|---|---|---|
| `git add PATH… && git commit -m …` (by pathspec, never a peer's file) | `actautoma commit --path P… -m …` (refuses a path that is not listed, keeps the session's attribution trailer, runs the pre-commit gate, never `--no-verify`) | actautoma (done, feat/commit-verb) |
| `git pull`, `git fetch && git merge --ff-only` | `actrepo pull` (fast-forward only; refuses when incoming files overlap a dirty file, naming it; never switches the branch of a checkout) | actrepo |
| `git push -u origin B` of one named branch from a worktree, the gate output kept, the result checked afterwards | `actrepo push --branch B` (today `actrepo push` takes a repository and its nested ones, clean tree only); the result is read back with `branch remote`, not from an exit code | actrepo |
| `git worktree add .worktrees/X -b BRANCH origin/main` | `actrepo worktree new BRANCH [--base main]` (done, actrepo #98: it also pushes the branch with `-u`, so the remote branch exists from creation; any `actautoma` verb that creates a branch, such as the planned `worktree add`, must do the same, owner 2026-11-27) | actrepo |
| `git fetch`, `git ls-remote --heads origin B` | `actrepo branch remote B` (is it pushed, at which sha) | actrepo |
| `git branch -d B`, `git worktree remove --force` (generated files only) | `actrepo worktree remove`, `actrepo branch delete --merged` | actrepo |
| `git worktree remove W && git branch -D B` for a never-pushed branch whose content `main` already holds (found 2026-11-27: `doctrine/session-close`, superseded by PR #73; `actrepo worktree prune` skips it because the branch is not contained in `origin/main`) | `actautoma worktree prune --superseded` (remove a clean worktree and its branch when every change of the branch is already in the base, checked by patch-id or tree diff; print the sha so `git reflog` can recover it; refuse a dirty worktree, naming the file) | actautoma |
| `rm .git/worktrees/W/index.lock` after a commit killed by the gate's queue wait left a stale lock (found 2026-11-27: Actit bump worktree, no process held it) | `actautoma unlock [--worktree W]` (remove `index.lock` only when no process holds it, checked with `lsof` and a running `git`; print the lock's age; refuse and name the holder otherwise) | actautoma |
| `git branch -D B` for the branch of a closed, superseded bump PR (found 2026-11-27: `chore/bump-actautoma-37219d6`, PR actrepo #95 closed for #97; `git worktree remove` worked, `actrepo worktree prune` keeps the branch) | `actautoma worktree prune --closed` (delete the local branch when its PR is closed and its worktree is gone; print the sha so `git reflog` can recover it) | actautoma |
| `gh pr view N --json state,mergeable,baseRefName` | `actrepo pr view N` | actrepo |
| `gh pr list --state open --json …` | `actrepo pr list` | actrepo |
| `gh pr edit N --base main` | `actrepo pr retarget N --base main` | actrepo |
| `gh pr close N --comment …` | `actrepo pr close N [--comment]` | actrepo |
| `git merge-tree --write-tree BASE HEAD` | `actrepo pr check N` (conflict dry run) | actrepo |
| merge `origin/main` in a detached worktree, fix, `git push origin HEAD:B` | `actautoma resolve-conflicts N` (worktree, merge, stop on conflict, push) | actautoma |
| push, `actrepo pr create` with the last subject, `actrepo pr merge --yes` (`ship.sh`) | `actautoma ship` (gates first, merge only where authorised) — built 2026-11-24: green gate + `MERGEABLE` + `CLEAN`; never in `acteble_api`, `melos_apps` | actautoma |
| stacked PRs squash-merged: retarget the tip, dry run, merge, close the contained ones | `actautoma merge-stack` | actautoma |
| `gh issue create`, `gh search issues`, `gh repo fork` for a problem found upstream | `actautoma report-upstream` | actautoma |
| `actrepo pr merge` then restore a peer's checkout it switched to `main` | `actrepo pr merge --keep-checkout` (never switch a checkout that is not the merger's) | actrepo |

Known defects seen while shipping (to fix, test first): `actrepo pr merge`
switched peers' checkouts to `main`; it reports "PR merged, but the local
update is incomplete" when the worktree holds untracked generated files and
leaves the worktree behind (nothing says which file blocks it).

### Done 2026-11-27: `rename-repo` and a case-only rename on macOS

Both halves of the 2026-11-26 bug are fixed. `binary::check` no longer refuses the rename as "already exists": `same_file` compares device and inode (test `a_case_only_rename_on_a_case_insensitive_disk_is_not_a_clash`). `git worktree repair` writes nothing when only the case changed (checked with an experiment: not even with an explicit path), so `worktree_links::relink` rewrites `.git/worktrees/<name>/gitdir` and each worktree's `.git` file after the move (tests in `src/worktree_links.rs`, wiring test `the_link_files_of_nested_worktrees_name_the_new_folder_after_the_move`). Not covered: a worktree whose folder is neither nested nor listed by `git worktree list`.

### Bulk case (owner, 2026-11-23): several pushes and several pulls at once

`actautoma sync-all` settles the case of 3 pushes and 3 pulls pending without a human sequencing them. It calls `actrepo sync` (the per-branch verb: push ahead, fast-forward pull behind, report diverged and dirty), then `resolve-conflicts` for each diverged branch, then `ship` where merging is authorised (`openacte` repositories only). Every item is attempted, each outcome is read back, one report line per item, non-zero exit when any item failed. Own ADR before code; full design in `actrepo/ROADMAP.md` ("Bulk case"). It needs `actrepo pull`, `push --branch` and `branch remote` first.

### Adaptive resolution (owner, 2026-11-25): a workflow reads the situation before it acts

A workflow of `actautoma` is not a fixed sequence of commands. When something
stands in its way, it diagnoses the situation, proposes a resolution plan, and
executes only the steps that are safe. Case that triggered this directive: a
`git push` waited eleven hours in the pre-push gate queue while the queue showed
no slot held and seven gates waiting; two pushes of the same branch were running
at once; a gate outlived its dead push; a gate that failed because the session
killed it was reported with exit code 0. Nothing said any of this; the owner and
the agent found it by hand, one `ps` at a time.

Before it acts, and again when it waits longer than expected, a workflow:

- **Reads the state.** Queue state (`actqueue state`: slots held, waiters, the
  position of its own job), running `git push` and gate processes of the same
  branch, the remote branch (`branch remote`), and the pull request if any.
- **Names duplicates.** Two pushes or two gates for one branch: it keeps the
  oldest one that is alive, names the other, and stops it only after saying so.
- **Names orphans.** A gate whose parent push is dead, a gate of a temporary
  directory, a waiter that makes no progress. It lists each with its pid, age
  and job name, and asks before stopping it (confirm before deleting).
- **Reads the result back.** A push is done when the branch is on the remote at
  the expected sha, never because an exit code was 0; a gate stopped by a signal
  is reported as stopped, not as failed or passed.
- **Proposes a plan.** One line per step (for example: push with `-u origin
  BRANCH`, open the pull request, do not merge), marks each step safe or needing
  confirmation, runs the safe ones, and reports what remains.
- **Never hides a stall.** Waiting beyond a threshold prints why (who is ahead,
  who holds the slot) instead of staying silent.

Scope: first `ship`, then `sync-all`, then every later workflow. It never
bypasses a gate (`--no-verify`), never kills a process it did not start unless
the owner confirms, and never merges where merging is not authorised. Own ADR
before code: what it may do alone, what stops it, how the threshold is set.
Tests first: a fixture with a duplicate push, an orphan gate and a stalled queue;
the plan must name each, and nothing may be stopped without confirmation.

### Order

1. Read-only verbs first (`pr view`, `pr list`, `branch remote`, `pr check`): additive, no risk.
2. Then the writing single verbs (`worktree add|remove`, `pr retarget|close`).
3. Then the `actautoma` workflows, each with its own ADR (what it may do alone, what stops it).
4. Then a rule in `~/.claude/AGENTS.md` (dotfiles) that makes raw `git`/`gh` the exception. The rule text is proposed in the dotfiles pull request of the same day.
