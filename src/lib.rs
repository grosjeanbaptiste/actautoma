//! actautoma — TODO: describe the crate.

pub mod binary;
pub mod branches;
pub mod catch_up;
pub mod clean;
pub mod cli;
pub mod family;
pub mod forge;
pub mod gh;
mod gh_backup;
pub mod glab;
mod glab_backup;
pub mod install_records;
pub mod move_dir;
pub mod process;
pub mod pulls;
pub mod realign_fs;
pub mod registry;
pub mod rename;
pub mod rename_file;
pub mod ship;

/// Moved to `actpath` (ADR 0001); re-exported so existing `actautoma::realign` users keep working.
pub use actpath::realign;

#[cfg(test)]
mod testing;

/// Compiles the README examples so the documented API cannot drift from the code.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
