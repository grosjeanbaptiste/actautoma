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
- 2026-11-16 (branch `fix/case-only-rename`): `rename-repo` refused a case-only rename (`OpenActe` -> `openacte`) on a case-insensitive disk with "already exists", because `new_dir` is `old_dir`. `rename::case_only` (same inode, names equal ignoring case) lets it through. Test: `a_case_only_rename_moves_the_folder_instead_of_refusing` (macOS only). 85 unit + 2 doc tests pass, clippy and fmt clean.
- 2026-11-16 (branch `feat/rename-binary`): `rename-repo` also renames the Cargo binary: root `Cargo.toml` [package]/[[bin]] name, `Cargo.lock` root entry (registry dependencies untouched) and the installed `BIN_DIR/OLD` (`--bin-dir`, default `$CARGO_HOME/bin`). Refuses before any change when `BIN_DIR/NEW` exists. Also renames workspace members (`dir/*` or literal; other wildcards refused before any change) and rewrites cargo's `.crates.toml`/`.crates2.json` records (`src/install_records.rs`, adds `serde_json`). 94 unit + 2 doc tests pass, clippy and fmt clean. Source: `src/binary.rs`, `src/rename.rs`.
- 2026-11-17 (branch `feat/rename-family`): `rename-repo` also renames the workspace family: members named `old` or `old-*` (package names, folders), and every mention in the moved folder (`old-x`, `old_x::`, `Old`, `OLD`, also `old-store-{}`, `OLD_TAG`: `realign::replace_words`, a match only has to stand apart from letters and digits; `older` is untouched). Folder clashes are refused in `check`, before the platform is touched. `actautoma rename-family OLD NEW [--dir DIR]` runs the pass alone for a folder already moved. Source: `src/family.rs`, `src/realign.rs`, `src/rename.rs`. 107 unit + 2 doc tests pass, clippy `-D warnings` and fmt clean. Applied to bioxus -> biorcrux. Known edges: CamelCase identifiers (`BioxusError`) are not matched; prose such as `old-ish` is rewritten. Slips noted: `replace_words` and its refactor of `replace` were written in one go with its test (not red first); `rename-family` now lives in `src/cli.rs` (after the merge of the CLI move) with 3 tests of its own.

