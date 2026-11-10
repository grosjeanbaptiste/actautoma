# actautoma

Rust library that handles the tools of repository-hosting platforms (`gh` for
GitHub, `glab` for GitLab, and others). The `gh` adapter exists; this README
states what exists today and grows with the API.

## Install

Needs Rust (`rustup`). `actautoma` is a library: it ships no binary, so there is
nothing to `cargo install`. Add it to another crate:

```text
cargo add --git https://github.com/grosjeanbaptiste/actautoma
```

From a checkout: `cargo test` builds and checks it.

## API

### Command line

None.

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

- `Forge` (in `forge`) is the port: `view_pr`, `create_pr`, `merge_pr`. One adapter per
  platform tool; `Gh` (GitHub, `gh`) is the first. `glab` and others plug in by
  implementing it.
- `Runner` is the seam to the process: `ProcessRunner` spawns it, a test supplies a fake.
- `Mergeable` is `Mergeable`, `Conflicting`, `Unknown` (the platform computes it lazily,
  ask again later) or `Other(text)` for a value this crate has no name for.
- Every failure is an `Error`: `Run` (tool missing, non-zero exit, with its stderr) or
  `Parse` (an answer of the wrong shape). Nothing is swallowed.

### Limits

Only the `gh` adapter exists. No `glab` yet. Nothing here depends on **actit** or
**actname**.

## Develop

```text
dotfiles heavy run --medium -- cargo test
dotfiles heavy run --medium -- cargo clippy --all-targets -- -D warnings
```
