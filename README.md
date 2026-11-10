# actautoma

Rust library. Purpose: handle the tools of repository-hosting platforms (`gh` for
GitHub, `glab` for GitLab, and others). Only `version()` exists so far; this
README states what exists today and grows with the API.

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

```rust
assert_eq!(actautoma::version(), env!("CARGO_PKG_VERSION"));
```

`version() -> &'static str` returns the crate version from `Cargo.toml`. It is
the only public item.

### Limits

No feature yet. Nothing here depends on **actit** or **actname**.

## Develop

```text
dotfiles heavy run --medium -- cargo test
dotfiles heavy run --medium -- cargo clippy --all-targets -- -D warnings
```
