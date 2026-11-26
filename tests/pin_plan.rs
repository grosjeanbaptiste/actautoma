//! `pin_plan::plan`: which repositories pin a dependency, in the order to bump them.

use actautoma::pin_plan::{plan, Consumer};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

fn root() -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("actautoma-pin-plan-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p); // a leftover of an earlier run: start empty
    fs::create_dir_all(&p).unwrap();
    p
}

/// A repository folder `folder` whose package is `package` and whose dependencies are `deps`
/// (name, rev), each a git dependency.
fn repo(root: &Path, folder: &str, package: &str, deps: &[(&str, &str)]) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let mut text =
        format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
    for (name, rev) in deps {
        text.push_str(&format!(
            "{name} = {{ git = \"https://github.com/o/{name}\", rev = \"{rev}\" }}\n"
        ));
    }
    fs::write(dir.join("Cargo.toml"), text).unwrap();
}

fn names(consumers: &[Consumer]) -> Vec<&str> {
    consumers.iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn a_consumer_is_found_with_its_old_revision() {
    let r = root();
    repo(&r, "actrepo", "actrepo", &[("actautoma", OLD)]);

    let found = plan(&r, "actautoma", NEW).unwrap();

    assert_eq!(names(&found), ["actrepo"]);
    assert_eq!(found[0].old_rev, OLD);
    assert!(!found[0].up_to_date);
    assert_eq!(found[0].dir, r.join("actrepo"));
}

#[test]
fn a_consumer_already_on_the_revision_is_up_to_date() {
    let r = root();
    repo(&r, "actrepo", "actrepo", &[("actautoma", NEW)]);

    let found = plan(&r, "actautoma", NEW).unwrap();

    assert!(found[0].up_to_date);
}

#[test]
fn a_folder_that_does_not_pin_the_dependency_is_left_out() {
    let r = root();
    repo(&r, "actrepo", "actrepo", &[("actautoma", OLD)]);
    repo(&r, "actname", "actname", &[("serde", OLD)]);
    fs::create_dir_all(r.join("docs")).unwrap(); // no Cargo.toml at all

    assert_eq!(names(&plan(&r, "actautoma", NEW).unwrap()), ["actrepo"]);
}

#[test]
fn a_consumer_comes_after_the_consumers_it_depends_on() {
    let r = root();
    // actrepo pins actit, which pins actautoma: actit goes first whatever the folder order.
    repo(
        &r,
        "actrepo",
        "actrepo",
        &[("actautoma", OLD), ("actit", OLD)],
    );
    repo(&r, "Actit", "actit", &[("actautoma", OLD)]);

    assert_eq!(
        names(&plan(&r, "actautoma", NEW).unwrap()),
        ["Actit", "actrepo"]
    );
}

#[test]
fn independent_consumers_are_listed_by_name() {
    let r = root();
    repo(&r, "b", "b", &[("actautoma", OLD)]);
    repo(&r, "a", "a", &[("actautoma", OLD)]);

    assert_eq!(names(&plan(&r, "actautoma", NEW).unwrap()), ["a", "b"]);
}

#[test]
fn a_cycle_is_an_error_naming_its_members() {
    let r = root();
    repo(&r, "a", "a", &[("actautoma", OLD), ("b", OLD)]);
    repo(&r, "b", "b", &[("actautoma", OLD), ("a", OLD)]);

    let e = plan(&r, "actautoma", NEW).unwrap_err().to_string();

    assert!(
        e.contains("cycle") && e.contains('a') && e.contains('b'),
        "{e}"
    );
}

#[test]
fn a_git_dependency_without_a_rev_is_an_error_naming_the_repository() {
    let r = root();
    let dir = r.join("actrepo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"actrepo\"\n\n[dependencies]\nactautoma = { git = \"https://x/actautoma\" }\n",
    )
    .unwrap();

    let e = plan(&r, "actautoma", NEW).unwrap_err().to_string();

    assert!(e.contains("actrepo") && e.contains("rev"), "{e}");
}

#[test]
fn an_unreadable_manifest_is_an_error_not_a_skip() {
    let r = root();
    fs::create_dir_all(r.join("broken/Cargo.toml")).unwrap(); // a folder where the file should be

    assert!(plan(&r, "actautoma", NEW).is_err());
}

#[test]
fn a_revision_that_is_not_a_full_hash_is_refused() {
    let r = root();
    repo(&r, "actrepo", "actrepo", &[("actautoma", OLD)]);

    let e = plan(&r, "actautoma", "abc").unwrap_err().to_string();

    assert!(e.contains("40"), "{e}");
}

#[test]
fn a_missing_root_is_an_error() {
    let r = root().join("absent");

    assert!(plan(&r, "actautoma", NEW).is_err());
}

#[test]
fn a_dependency_from_a_registry_is_not_a_git_pin() {
    let r = root();
    let dir = r.join("actrepo");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"actrepo\"\n\n[dependencies]\nactautoma = \"1\"\n",
    )
    .unwrap();

    assert!(plan(&r, "actautoma", NEW).unwrap().is_empty());
}

#[test]
fn a_dependency_whose_name_only_starts_like_the_wanted_one_is_not_a_pin() {
    let r = root();
    repo(&r, "actrepo", "actrepo", &[("actautoma-extra", OLD)]);

    assert!(plan(&r, "actautoma", NEW).unwrap().is_empty());
}

#[test]
fn a_consumer_names_its_package_and_the_revision_it_pins_of_any_dependency() {
    let r = root();
    repo(
        &r,
        "actrepo-folder",
        "actrepo",
        &[("actautoma", OLD), ("actit", NEW)],
    );

    let found = plan(&r, "actautoma", NEW).unwrap();

    assert_eq!(found[0].package(), "actrepo");
    assert_eq!(found[0].pinned_rev("actit").unwrap().as_deref(), Some(NEW));
    assert_eq!(
        found[0].pinned_rev("actautoma").unwrap().as_deref(),
        Some(OLD)
    );
    assert_eq!(found[0].pinned_rev("serde").unwrap(), None);
}

#[test]
fn a_repository_of_emvi_is_never_a_consumer() {
    let r = root();
    repo(&r, "emvi-app", "emvi-app", &[("actautoma", OLD)]);
    repo(&r, "actrepo", "actrepo", &[("actautoma", OLD)]);

    let found = plan(&r, "actautoma", NEW).unwrap();

    assert_eq!(names(&found), ["actrepo"]);
}
