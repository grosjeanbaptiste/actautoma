# actautoma

Rust library that handles the tools of repository-hosting platforms (`gh` for
GitHub, `glab` for GitLab, and others). The `gh` adapter exists; this README
states what exists today and grows with the API.

## Install

Needs Rust (`rustup`). `actautoma` is a library with one small binary (`rename-repo`, below).
On this machine: `dotfiles project install actautoma`. Add the library to another crate:

```text
cargo add --git https://github.com/grosjeanbaptiste/actautoma
```

From a checkout: `cargo test` builds and checks it.

## API

### Command line

```text
actautoma rename-repo OWNER/OLD NEW [--dir OLD_DIR] [--root DIR]... [--children DIR]... [--local-only] [--dry-run]
```

Renames `OWNER/OLD` to `NEW` on GitHub (`gh`), repoints the local `origin`, moves `OLD_DIR`
(default: the current folder) to its sibling `NEW`, rewrites the old name in the text files
under each `--root` and renames the children of each `--children` folder named after the
encoded old folder. Every check runs before the first change. `--local-only` skips the
platform (it already has the new name); `--dry-run` prints the rewrites and changes nothing.
On this machine, `dotfiles project rename OWNER/OLD NEW` supplies the roots.

### Library

```rust,no_run
use actautoma::{forge::{Forge, NewPullRequest}, gh::Gh, process::ProcessRunner};

let forge = Gh::new(ProcessRunner).in_dir("."); // `.program("/opt/gh")` picks another binary
let url = forge.create_pr(&NewPullRequest {
    base: "main".into(),
    head: "feat/x".into(),
    title: "Add x".into(),
    body: "Why x.".into(),
})?;
let pr = forge.view_pr(None)?; // PullRequest { number, title, head, base, author, mergeable, review, merge_state }
if pr.mergeable == actautoma::forge::Mergeable::Mergeable {
    forge.merge_pr(pr.number)?; // merges and deletes the branch
}
# Ok::<(), actautoma::forge::Error>(())
```

- `Forge` (in `forge`) is the port: pull requests (`view_pr`, `create_pr`, `merge_pr`),
  repositories (`view_repo`, `set_default_branch`, `create_repo`) and `available`, which
  fails when the platform tool is missing. Name lookups: `user` (the authenticated login),
  `repo_exists(owner, name)` (a platform error is never `false`) and `search_repos(text)`
  (`RepoSearch { total, names }`, first page). They default to `Error::Unsupported`. One adapter per
  platform tool: `Gh` (GitHub, `gh`) and `Glab` (GitLab, `glab`; a pull request is a merge
  request). Others plug in by implementing the trait. `Glab::new(ProcessRunner)` is a drop-in
  for `Gh::new(ProcessRunner)`.
- `Runner` is the seam to the process: `ProcessRunner` spawns it, a test supplies a fake.
- `Mergeable` is `Mergeable`, `Conflicting`, `Unknown` (the platform computes it lazily,
  ask again later) or `Other(text)` for a value this crate has no name for.
- Every failure is an `Error`: `Run` (tool missing, non-zero exit, with its stderr),
  `Parse` (an answer of the wrong shape) or `Unsupported` (the adapter lacks the operation,
  nothing was run). Nothing is swallowed.

### Realign after a rename

`Forge::rename_repo(slug, new_name)` renames the repository on the platform (`Gh` only; `Glab`
answers `Error::Unsupported`). The written references are then realigned by `realign`:

```rust,no_run
use actautoma::realign::{for_repo, replace};
use actautoma::realign_fs::{realign_tree, rename_children};
use std::path::Path;

let home = Path::new("/Users/me");
let renames = for_repo(
    "me", "polytech-corpus", "corpus",
    Path::new("/Users/me/Source/polytech-corpus"), Path::new("/Users/me/Source/corpus"), home,
);
let (_text, _count) = replace("git@github.com:me/polytech-corpus.git", &renames);
let report = realign_tree(Path::new("/Users/me/machine"), &renames)?; // rewrites text files
let _moved = rename_children(Path::new("/Users/me/claude-memory/projects"), &renames)?;
println!("{} files rewritten, {} not text, {} links", report.changed.len(), report.not_text.len(), report.links.len());
# Ok::<(), actautoma::forge::Error>(())
```

- `for_repo` lists the literal forms a rename leaves behind: `owner/old`, the directory, its
  `~/` form under `home`, and the directory encoded as a file name (`/` becomes `-`).
- `replace` only rewrites a whole name: `polytech` is not touched inside `polytech-corpus`.
  A `.git` suffix and a sentence period do not hide a match.
- `realign_tree` rewrites text files under a root and skips `.git`, `.venv`, `target`,
  `node_modules`, `.lake` and caches. A non-UTF-8 file and a symlink are listed in the
  `Report`, never skipped quietly. `rename_children` renames direct children whose whole name
  is a `from`, and refuses to overwrite an existing target.

### Limits

`Gh` and `Glab` exist; no Gitea or Bitbucket yet. Nothing here depends on **actit** or
**actname**.

`Glab` does not implement `user`, `repo_exists` and `search_repos`: they answer
`Error::Unsupported`.

`Glab` is tested against a scripted runner only, never against a live GitLab: its JSON field
names (`iid`, `detailed_merge_status`, `has_conflicts`, `path_with_namespace`,
`default_branch`) come from the GitLab API documentation. Its differences from `Gh`:
`PullRequest::review` is empty (`glab mr view` reports no review decision), and
`create_repo` has no push flag in `glab`, so a requested push is a separate
`git push -u origin --all` run in the source directory.

## Develop

```text
dotfiles heavy run --medium -- cargo test
dotfiles heavy run --medium -- cargo clippy --all-targets -- -D warnings
```
