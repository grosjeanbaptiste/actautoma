//! actautoma — TODO: describe the crate.

pub mod forge;
pub mod gh;
pub mod process;

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
