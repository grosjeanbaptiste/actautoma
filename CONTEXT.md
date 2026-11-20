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

- 2026-11-19 (branch `feat/rename-file`): `actautoma rename-file OLD NEW [--root DIR]... [--replace FROM TO]...` renames one tracked file with `git mv` and rewrites its references (owner: every rename belongs to actautoma). 11 tests incl. real git. The branches `feat/rename-binary`, `fix/case-only-rename`, `feat/rename-family`, `feat/cli-dispatch`, `feat/rename-into` and `feat/rename-registry` above are merged (PRs #10-#15). Source: `src/rename_file.rs`.

- 2026-11-20 (branch `feat/move-already-moved`): `actautoma move DIR --into PARENT --already-moved` repairs a folder that something else already moved: nothing is moved, `DIR` is where it was, the repositories are scanned at the new place, linked worktrees re-linked with `git worktree repair`, every reference rewritten as `move` does. Refuses when `DIR` still exists or the new folder is missing. 4 tests with real git (156 unit + 2 doc tests pass, clippy `-D warnings` clean). Motivation: `~/Source/openacte` was moved to `~/Source/Acteble/openacte` by hand and cargo's `.crates2.json` still names the old paths (`actinstall` skips those installs). Source: `src/move_dir.rs`.
- 2026-11-20 (uncommitted, branch `main`): `actautoma move DIR --into PARENT [--root] [--children] [--registry] [--cargo-home] [--dry-run]` (`src/move_dir.rs`) moves a repository or a folder of repositories in one `rename`; the repositories and linked worktrees are found by scanning, the worktrees re-linked by `git worktree repair <paths>` run from each main clone, every written path rewritten (absolute, `~`, `$HOME`, encoded, shared tail for variable-based paths, `../x` recomputed when the target exists), the surrounding folders, `~/.claude/projects`, cargo records and `~/.claude` files taken without being named, the new parent's `.gitignore` updated, files over 16 MiB never read (`src/realign_fs.rs`). 152 unit + 2 doc tests pass (real `git worktree` in temp folders), clippy `-D warnings` and fmt clean. Defect found by a test: macOS `/var` is `/private/var`, git prints the latter, so `dir` and `into` are canonicalized first. `--dry-run` on `~/Source/Acteble/openacte` into `~/Source/Acteble` found 29-30 repositories; the real move has NOT been run (sessions open in the folder, 2026-11-19). Not verified: `rename-repo` runs `git worktree repair` from the worktree, `git` documents running it from the main clone with the worktree paths (not tested against real git). Pending: `repo move` in actrepo needs this committed and the actrepo pin bumped (`repo actautoma move ...` works once the pin carries it).
