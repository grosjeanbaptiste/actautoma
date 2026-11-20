//! actautoma — TODO: describe the crate.

pub mod binary;
pub mod cli;
pub mod family;
pub mod forge;
pub mod gh;
pub mod glab;
pub mod install_records;
pub mod move_dir;
pub mod process;
pub mod realign;
pub mod realign_fs;
pub mod registry;
pub mod rename;
pub mod rename_file;

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
