//! `branches::classify` on real repositories: one branch per case.

use actautoma::branches::{classify, Branch, Class};
use actautoma::process::ProcessRunner;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    write(dir, file, text);
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "actautoma-branches-{label}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A repository on `main` with `a.txt`, and an `origin` that holds it.
fn repo() -> PathBuf {
    let root = scratch("repo");
    let origin = root.join("origin.git");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&work, &["init", "-q", "-b", "main"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    commit(&work, "a.txt", "one\ntwo\nthree\n", "base");
    git(&work, &["push", "-q", "origin", "main"]);
    work
}

fn branch_from_main(dir: &Path, name: &str) {
    git(dir, &["checkout", "-q", "-b", name, "main"]);
}

fn find<'a>(all: &'a [Branch], name: &str) -> &'a Branch {
    all.iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no {name} in {all:?}"))
}

#[test]
fn every_case_gets_its_class() {
    let dir = repo();

    branch_from_main(&dir, "feat/merged");
    commit(&dir, "m.txt", "m\n", "merged work");
    git(&dir, &["checkout", "-q", "main"]);
    git(
        &dir,
        &["merge", "-q", "--no-ff", "-m", "merge", "feat/merged"],
    );

    branch_from_main(&dir, "feat/superseded");
    commit(&dir, "s.txt", "same\n", "same work, branch side");
    git(&dir, &["checkout", "-q", "main"]);
    commit(&dir, "s.txt", "same\n", "same work, main side");

    branch_from_main(&dir, "feat/mergeable");
    commit(&dir, "new.txt", "new\n", "new work");
    git(&dir, &["checkout", "-q", "main"]);

    branch_from_main(&dir, "feat/conflicting");
    commit(&dir, "a.txt", "one\nBRANCH\nthree\n", "branch edit");
    git(&dir, &["checkout", "-q", "main"]);
    commit(&dir, "a.txt", "one\nMAIN\nthree\n", "main edit");

    let all = classify(&ProcessRunner, &dir, "main").unwrap();

    assert_eq!(find(&all, "feat/merged").class, Class::Merged);
    assert_eq!(find(&all, "feat/merged").ahead, 0);
    assert_eq!(find(&all, "feat/superseded").class, Class::Superseded);
    assert_eq!(find(&all, "feat/superseded").ahead, 1);
    assert_eq!(find(&all, "feat/mergeable").class, Class::Mergeable);
    assert_eq!(
        find(&all, "feat/conflicting").class,
        Class::Conflicting(vec!["a.txt".to_string()])
    );
    assert!(
        all.iter().all(|b| b.name != "main"),
        "the base is not classified: {all:?}"
    );
}

#[test]
fn a_remote_only_branch_is_named_with_its_remote() {
    let dir = repo();
    branch_from_main(&dir, "feat/remote");
    commit(&dir, "r.txt", "r\n", "remote work");
    git(&dir, &["push", "-q", "origin", "feat/remote"]);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["branch", "-q", "-D", "feat/remote"]);
    git(&dir, &["fetch", "-q", "origin"]);

    let all = classify(&ProcessRunner, &dir, "main").unwrap();

    assert_eq!(find(&all, "origin/feat/remote").class, Class::Mergeable);
    assert!(
        all.iter()
            .all(|b| b.name != "origin/main" && b.name != "origin/HEAD"),
        "{all:?}"
    );
}

#[test]
fn an_unknown_base_is_an_error_not_an_empty_list() {
    let dir = repo();
    assert!(classify(&ProcessRunner, &dir, "no-such-base").is_err());
}
