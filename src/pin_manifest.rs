//! Read the `rev` of one git dependency in a `Cargo.toml`.
//!
//! Moved here from `actrepo/src/pin/manifest.rs` (ADR 0002, slice 2 removes that copy).

use crate::forge::Error;

/// The line declaring `dep = { git = …, rev = "…" }`.
fn line_of<'a>(toml: &'a str, dep: &str) -> Option<&'a str> {
    toml.lines().find(|l| {
        let t = l.trim_start();
        t.strip_prefix(dep)
            .is_some_and(|r| r.trim_start().starts_with('=') && t.contains("git"))
    })
}

/// The quoted value after `rev` on the line.
fn rev_in(line: &str) -> Option<&str> {
    let at = line.find("rev")?;
    let open = at + line[at..].find('"')? + 1;
    Some(&line[open..open + line[open..].find('"')?])
}

/// The pinned revision; `None` when `dep` is not a git dependency here.
///
/// # Errors
/// When `dep` is a git dependency without a `rev`: there is nothing to pin.
pub fn rev_of(toml: &str, dep: &str) -> Result<Option<String>, Error> {
    let Some(line) = line_of(toml, dep) else {
        return Ok(None);
    };
    match rev_in(line) {
        Some(rev) => Ok(Some(rev.to_string())),
        None => Err(Error::Run(format!(
            "{dep} is a git dependency without a rev: nothing to pin"
        ))),
    }
}

/// Whether `rev` is a full 40-character commit hash.
#[must_use]
pub fn is_full_hash(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}
