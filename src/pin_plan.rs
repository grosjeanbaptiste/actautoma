//! Which repositories pin a dependency, and in which order to bump them (ADR 0002, slice 1).
//!
//! A consumer is a direct sub-folder of the root whose `Cargo.toml` pins the dependency as a
//! git `rev`. A consumer that itself pins another consumer comes after it, so that it pins a
//! revision that already exists. Nothing is written and no git command runs.

use crate::binary::package_name;
use crate::forge::Error;
use crate::pin_manifest::{is_full_hash, rev_of};
use std::fs;
use std::path::{Path, PathBuf};

/// A repository that pins the dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    /// The folder name.
    pub name: String,
    pub dir: PathBuf,
    /// The revision it pins today.
    pub old_rev: String,
    /// It already pins the wanted revision.
    pub up_to_date: bool,
    package: String,
    manifest: String,
}

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("{what} {}: {e}", path.display()))
}

/// The consumers of `dep` under `root`, with their manifests read.
fn consumers(root: &Path, dep: &str, rev: &str) -> Result<Vec<Consumer>, Error> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| io(root, "read", &e))? {
        let path = entry.map_err(|e| io(root, "read", &e))?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    let mut found = Vec::new();
    for dir in dirs {
        let file = dir.join("Cargo.toml");
        if !file.exists() {
            continue; // not a Rust repository: nothing to pin there
        }
        let name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let manifest = fs::read_to_string(&file).map_err(|e| io(&file, "read", &e))?;
        let old = rev_of(&manifest, dep).map_err(|e| Error::Run(format!("{name}: {e}")))?;
        if let Some(old_rev) = old {
            found.push(Consumer {
                up_to_date: old_rev == rev,
                package: package_name(&manifest).unwrap_or_else(|| name.clone()),
                name,
                dir,
                old_rev,
                manifest,
            });
        }
    }
    Ok(found)
}

/// Whether `c` pins the package of `other`.
fn pins(c: &Consumer, other: &Consumer) -> Result<bool, Error> {
    Ok(c.name != other.name
        && rev_of(&c.manifest, &other.package)
            .map_err(|e| Error::Run(format!("{}: {e}", c.name)))?
            .is_some())
}

/// The consumers of `dep` under `root`, ordered so that each follows those it pins.
///
/// # Errors
/// When `rev` is not a full hash, `root` or a manifest cannot be read, a git dependency has no
/// `rev`, or the consumers pin each other in a cycle.
pub fn plan(root: &Path, dep: &str, rev: &str) -> Result<Vec<Consumer>, Error> {
    if !is_full_hash(rev) {
        return Err(Error::Run(format!(
            "{rev:?} is not a full 40-character commit hash"
        )));
    }
    let mut left = consumers(root, dep, rev)?;
    let mut ordered = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let mut ready = None;
        for (i, c) in left.iter().enumerate() {
            let mut blocked = false;
            for other in &left {
                blocked |= pins(c, other)?;
            }
            if !blocked {
                ready = Some(i);
                break;
            }
        }
        let Some(i) = ready else {
            let members: Vec<&str> = left.iter().map(|c| c.name.as_str()).collect();
            return Err(Error::Run(format!(
                "cycle among consumers of {dep}: {}",
                members.join(", ")
            )));
        };
        ordered.push(left.remove(i));
    }
    Ok(ordered)
}
