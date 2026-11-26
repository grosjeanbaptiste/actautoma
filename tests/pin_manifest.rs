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

const TAGGED: &str = "[dependencies]\nactconf = { git = \"https://x/actconf\", tag = \"v0.4.0\", default-features = false }\nactit = { git = \"ssh://x/Actit.git\", rev = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }\n";

#[test]
fn a_tag_pin_is_rewritten_to_the_new_tag_and_nothing_else() {
    let out = with_rev(TAGGED, "actconf", "v0.5.0").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out, Some(TAGGED.replacen("v0.4.0", "v0.5.0", 1)));
}

#[test]
fn the_pin_of_a_tag_dependency_is_its_tag() {
    use actautoma::pin_manifest::rev_of;
    assert_eq!(rev_of(TAGGED, "actconf").unwrap(), Some("v0.4.0".into()));
}

#[test]
fn a_tag_is_a_name_without_quote_or_space() {
    for bad in ["", "v0 .5", "v0\"5"] {
        assert!(with_rev(TAGGED, "actconf", bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_rev_pin_still_wants_a_full_hash_even_next_to_a_tag_pin() {
    assert!(with_rev(TAGGED, "actit", "v0.5.0").is_err());
}

#[test]
fn a_commit_hash_is_never_written_as_a_tag() {
    assert!(with_rev(TAGGED, "actconf", B).is_err());
}

#[test]
fn the_pin_key_is_found_by_its_whole_name_wherever_it_stands_on_the_line() {
    use actautoma::pin_manifest::rev_of;
    for line in [
        format!("actit = {{git=\"u\",rev=\"{A}\"}}"),
        format!("actit = {{rev=\"{A}\", git=\"u\"}}"),
        format!("actit = {{ git = \"u\", xrev = \"zz\", rev = \"{A}\" }}"),
        format!("actit = {{ git = \"u\", rev_note = \"zz\", rev = \"{A}\" }}"),
        format!("actit = {{ git = \"u\", rev\t= \"{A}\" }}"),
    ] {
        assert_eq!(rev_of(&line, "actit").unwrap(), Some(A.into()), "{line}");
    }
}
