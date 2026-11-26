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

/// How a git dependency is pinned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rev,
    Tag,
}

/// The key (`rev` or `tag`) of the line and the byte range of its quoted value.
fn pin_span(line: &str) -> Option<(Kind, usize, usize)> {
    [("rev", Kind::Rev), ("tag", Kind::Tag)]
        .into_iter()
        .find_map(|(key, kind)| {
            let at = line.match_indices(key).map(|(i, _)| i).find(|&i| {
                let before = line[..i].chars().next_back();
                let after = line[i + key.len()..].trim_start();
                before.is_some_and(|c| c.is_whitespace() || c == '{' || c == ',')
                    && after.starts_with('=')
            })?;
            let open = at + line[at..].find('"')? + 1;
            Some((kind, open, open + line[open..].find('"')?))
        })
}

fn unpinned(dep: &str) -> Error {
    Error::Run(format!(
        "{dep} is a git dependency without a rev or a tag: nothing to pin"
    ))
}

/// The text with `dep` pinned to `value`, every other byte kept; `None` when `dep` is not a git
/// dependency here. A `rev` pin wants a full commit hash, a `tag` pin a tag name.
///
/// # Errors
/// When `value` does not suit the pin, or `dep` is a git dependency with neither `rev` nor `tag`.
pub fn with_rev(toml: &str, dep: &str, value: &str) -> Result<Option<String>, Error> {
    let Some(line) = line_of(toml, dep) else {
        return Ok(None);
    };
    let Some((kind, a, b)) = pin_span(line) else {
        return Err(unpinned(dep));
    };
    match kind {
        Kind::Rev if !is_full_hash(value) => {
            return Err(Error::Run(format!(
                "{value:?} is not a full 40-character commit hash"
            )))
        }
        Kind::Tag if !is_tag_name(value) => {
            return Err(Error::Run(format!("{value:?} is not a tag name")))
        }
        _ => {}
    }
    let new = format!("{}{value}{}", &line[..a], &line[b..]);
    Ok(Some(toml.replacen(line, &new, 1)))
}

/// The pinned revision or tag; `None` when `dep` is not a git dependency here.
///
/// # Errors
/// When `dep` is a git dependency with neither `rev` nor `tag`: there is nothing to pin.
pub fn rev_of(toml: &str, dep: &str) -> Result<Option<String>, Error> {
    let Some(line) = line_of(toml, dep) else {
        return Ok(None);
    };
    match pin_span(line) {
        Some((_, a, b)) => Ok(Some(line[a..b].to_string())),
        None => Err(unpinned(dep)),
    }
}

fn is_tag_name(value: &str) -> bool {
    !value.is_empty()
        && !is_full_hash(value)
        && !value.chars().any(|c| c.is_whitespace() || c == '"')
}

/// Whether `rev` is a full 40-character commit hash.
#[must_use]
pub fn is_full_hash(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}
