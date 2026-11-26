//! `pin_manifest::with_rev`: rewrite one git dependency's `rev`, every other byte kept.

use actautoma::pin_manifest::with_rev;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn toml() -> String {
    format!(
        "[dependencies]\nactit = {{ git = \"ssh://x/Actit.git\", rev = \"{A}\" }}\nactautoma = {{ git = \"https://x/actautoma\", rev = \"{A}\" }}\nanyhow = \"1\"\n"
    )
}

#[test]
fn only_the_named_dependency_is_rewritten() {
    let out = with_rev(&toml(), "actit", B).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out, Some(toml().replacen(A, B, 1)));
}

#[test]
fn rewriting_an_absent_or_non_git_dependency_changes_nothing() {
    assert!(matches!(with_rev(&toml(), "serde", B), Ok(None)));
    assert!(matches!(with_rev(&toml(), "anyhow", B), Ok(None)));
}

#[test]
fn a_prefix_of_another_name_is_not_that_dependency() {
    assert!(matches!(with_rev(&toml(), "act", B), Ok(None)));
}

#[test]
fn a_rev_that_is_not_a_full_hash_is_refused() {
    for bad in ["", "abc", "main", &"z".repeat(40)] {
        assert!(with_rev(&toml(), "actit", bad).is_err(), "{bad}");
    }
}

#[test]
fn a_git_dependency_without_a_rev_is_an_error() {
    let t = "actit = { git = \"ssh://x\", branch = \"main\" }\n";
    assert!(with_rev(t, "actit", B).is_err());
}

#[test]
fn a_dependency_already_on_the_rev_is_returned_unchanged() {
    assert_eq!(
        with_rev(&toml(), "actit", A).unwrap_or_else(|e| panic!("{e}")),
        Some(toml())
    );
}
