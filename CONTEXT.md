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

- 2026-11-20: `actautoma move DIR --into PARENT [--root] [--children] [--fleet] [--registry] [--cargo-home] [--dry-run]` (`src/move_dir.rs`, commit 7d405c5 + the fixes below, uncommitted) moves a repository or a folder of repositories in one `rename`. Repositories and linked worktrees are found by scanning; worktrees re-linked with `git worktree repair <paths>` run from each main clone after the move; every written path rewritten (absolute, `~`, `$HOME`, encoded, shared tail for variable-based paths, `../x` only when the file or its target moved and the target exists); around the folder only the files git knows are rewritten (`git ls-files -co --exclude-standard`, `--fleet` names the folders, default: the parent); `~/.claude/projects` children (the folder's and each repository's), cargo's records and the user-wide `~/.claude` files follow; the new parent's `.gitignore` gains `/name/`; files over 16 MiB are named, never read; a read-only file is named (`not writable, still to fix`), the pass goes on; a move cut short is finished by running the same command again. 158 unit + 2 doc tests (real `git worktree` in temp folders), clippy `-D warnings` and fmt clean.
- 2026-11-20: `~/Source/openacte` was moved to `~/Source/Acteble/openacte` (35 repositories). The first run died on a read-only vendored file of an unrelated project (`Emvi/.../Pods/BoringSSL-GRPC`) after the folder had moved; it also exposed the whole-`~/Source` walk (170 GB) and a `../` pass that rewrote unmoved files. Fixed test-first, then the same command resumed the move. Checked afterwards: no `Source/openacte` left in tracked or untracked-not-ignored files of `~/Source/dotfiles` and `~/Source/Acteble`, worktrees usable, cargo records and `~/.claude/projects` renamed, `/openacte/` in `Acteble/.gitignore`. Open: a live Claude session whose cwd was the old folder recreates `-Users-...-Source-openacte` under `~/.claude/projects`; `rename-repo` runs `git worktree repair` from the worktree, git documents running it from the main clone with the worktree paths (not tested against real git); `repo move` in actrepo waits for Actit, actdevice and actmake to pin the same actautoma commit as actrepo.
- 2026-11-20 (branch `test/move-resumes-a-folder-moved-by-hand`): tests that pin the case of a folder moved by hand (references rewritten, an outside linked worktree relinked, refusal when neither folder exists). `move` already handles it through `resumed` (dir gone, new folder present, commit 80a5cfe), no flag needed; the earlier `--already-moved` branch is superseded. 161 unit + 2 doc tests pass, clippy `-D warnings` clean. Real use after a hand move: `actautoma move OLD_DIR --into NEW_PARENT --cargo-home ~/.cargo`. Source: `src/move_dir.rs`.
