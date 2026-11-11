# actautoma — state as of 2026-11-11

The verified, current state of this project. Every session reads this file
first and updates it after any act that changes the state (a send, a merge,
a release, a decision, a reply received). Put the date of the update in the
title. Each fact names its source (file, commit, message) so it can be
re-checked; anything not yet verified is marked "unverified". No secrets,
tokens or personal identifiers here.

## Deadlines

| Date | What | Status |
|---|---|---|
| <YYYY-MM-DD> | <deliverable or deadline> | <done / pending — what is missing> |

## Waiting on

- <who> — <what was asked, when, through what> — <follow-up date>

## Current state

- `Forge::rename_repo` (`Gh` only; `Glab` is `Unsupported`) and the `realign` / `realign_fs` modules exist on branch `feat/realign-after-rename`, not committed: 79 unit tests and the README doctests pass, clippy `-D warnings` clean (run 2026-11-11).
- `rename` module and the `actautoma rename-repo` binary (smoke-tested in a temp repo, `--local-only`); `dotfiles project rename` (`~/dotfiles/scripts/project-rename.sh`, 8 bats tests) calls it with this machine's roots. Both uncommitted. Not yet run on a real GitHub rename; the binary is not yet installed on PATH (`dotfiles project install actautoma` needs a committed tree).

## Decisions

- <YYYY-MM-DD> — <decision taken by the owner> — <why, if given>

## Derived state

<commands that recompute state from the repository instead of re-reading
it, e.g. `scripts/status.py` — prefer a script whose output is a pure
function of tracked files>
