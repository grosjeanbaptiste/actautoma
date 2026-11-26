//! Read and rewrite the `rev` of one git dependency in a `Cargo.toml`.
//!
//! Moved here from `actrepo/src/pin/manifest.rs` (ADR 0002, slice 2; `actrepo` drops its copy).

use crate::forge::Error;

/// The line declaring `dep = { git = …, rev = "…" }`.
fn line_of<'a>(toml: &'a str, dep: &str) -> Option<&'a str> {
    toml.lines().find(|l| {
        let t = l.trim_start();
        t.strip_prefix(dep)
            .is_some_and(|r| r.trim_start().starts_with('=') && t.contains("git"))
    })
}

/// Byte range of the quoted value after `rev` on the line.
fn rev_span(line: &str) -> Option<(usize, usize)> {
    let at = line.find("rev")?;
    let open = at + line[at..].find('"')? + 1;
    Some((open, open + line[open..].find('"')?))
}

/// The quoted value after `rev` on the line.
fn rev_in(line: &str) -> Option<&str> {
    rev_span(line).map(|(a, b)| &line[a..b])
}

/// The text with `dep` pinned to `rev`, every other byte kept; `None` when `dep` is not a git
/// dependency here.
///
/// # Errors
/// When `rev` is not a full 40-character hash, or `dep` is a git dependency without a `rev`.
pub fn with_rev(toml: &str, dep: &str, rev: &str) -> Result<Option<String>, Error> {
    if !is_full_hash(rev) {
        return Err(Error::Run(format!(
            "{rev:?} is not a full 40-character commit hash"
        )));
    }
    let Some(line) = line_of(toml, dep) else {
        return Ok(None);
    };
    let Some((a, b)) = rev_span(line) else {
        return Err(Error::Run(format!(
            "{dep} is a git dependency without a rev: nothing to pin"
        )));
    };
    let new = format!("{}{rev}{}", &line[..a], &line[b..]);
    Ok(Some(toml.replacen(line, &new, 1)))
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
