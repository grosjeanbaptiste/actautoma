//! `clean::clean` on scratch repositories: merged work goes, everything else stays.

use actautoma::clean::{clean, Step};
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

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// A repository on `main` and an `origin` that holds it.
fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "actautoma-clean-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&work, &["init", "-q", "-b", "main"]);
    git(
        &work,
        &[
            "remote",
            "add",
            "origin",
            root.join("origin.git").to_str().unwrap(),
        ],
    );
    commit(&work, "a.txt", "base\n", "base");
    git(&work, &["push", "-q", "origin", "main"]);
    work
}

/// A branch with its own worktree and one commit, pushed, then merged into `main`.
fn merged_with_worktree(dir: &Path, name: &str) -> PathBuf {
    let wt = dir.join(".worktrees").join(name);
    git(
        dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            name,
            wt.to_str().unwrap(),
            "main",
        ],
    );
    commit(
        &wt,
        &format!("{}.txt", name.replace('/', "-")),
        "x\n",
        "work",
    );
    git(&wt, &["push", "-q", "-u", "origin", name]);
    git(dir, &["merge", "-q", "--no-ff", "-m", "merge", name]);
    git(dir, &["push", "-q", "origin", "main"]);
    wt
}

fn has_local(dir: &Path, name: &str) -> bool {
    !git_out(dir, &["branch", "--list", name]).trim().is_empty()
}

fn has_remote(dir: &Path, name: &str) -> bool {
    !git_out(dir, &["ls-remote", "--heads", "origin", name])
        .trim()
        .is_empty()
}

#[test]
fn a_merged_pushed_branch_loses_its_worktree_and_both_refs() {
    let dir = repo();
    let wt = merged_with_worktree(&dir, "feat/done");

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(!wt.exists(), "worktree still there");
    assert!(!has_local(&dir, "feat/done"));
    assert!(!has_remote(&dir, "feat/done"));
    assert!(steps.contains(&Step::Removed {
        branch: "feat/done".into()
    }));
}

#[test]
fn a_dirty_worktree_of_a_merged_branch_is_kept() {
    let dir = repo();
    let wt = merged_with_worktree(&dir, "feat/dirty");
    std::fs::write(wt.join("scratch.txt"), "mine\n").unwrap();

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(wt.exists());
    assert!(has_local(&dir, "feat/dirty"));
    assert!(steps
        .iter()
        .any(|s| matches!(s, Step::Kept { branch, .. } if branch == "feat/dirty")));
}

#[test]
fn a_branch_never_pushed_with_no_commit_is_kept() {
    let dir = repo();
    git(&dir, &["branch", "feat/just-cut", "main"]);

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(has_local(&dir, "feat/just-cut"));
    assert!(steps
        .iter()
        .any(|s| matches!(s, Step::Kept { branch, .. } if branch == "feat/just-cut")));
}

#[test]
fn a_branch_with_unmerged_work_is_left_alone() {
    let dir = repo();
    git(&dir, &["checkout", "-q", "-b", "feat/open", "main"]);
    commit(&dir, "o.txt", "o\n", "open work");
    git(&dir, &["push", "-q", "-u", "origin", "feat/open"]);
    git(&dir, &["checkout", "-q", "main"]);

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(has_local(&dir, "feat/open"));
    assert!(has_remote(&dir, "feat/open"));
    assert!(steps.is_empty(), "{steps:?}");
}

#[test]
fn a_dry_run_reports_and_deletes_nothing() {
    let dir = repo();
    let wt = merged_with_worktree(&dir, "feat/dry");

    let steps = clean(&ProcessRunner, &dir, "main", false).unwrap();

    assert!(wt.exists());
    assert!(has_local(&dir, "feat/dry"));
    assert!(has_remote(&dir, "feat/dry"));
    assert!(steps.contains(&Step::Removed {
        branch: "feat/dry".into()
    }));
}

#[test]
fn a_branch_never_pushed_whose_tree_is_the_base_is_removed_with_its_worktree() {
    let dir = repo();
    let wt = dir.join(".worktrees/feat/same");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/same",
            wt.to_str().unwrap(),
            "main",
        ],
    );
    commit(&wt, "f.txt", "same\n", "work, never pushed");
    commit(
        &dir,
        "f.txt",
        "same\n",
        "the same work, landed on main by another way",
    );
    git(&dir, &["push", "-q", "origin", "main"]);

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(!wt.exists(), "worktree still there");
    assert!(!has_local(&dir, "feat/same"));
    assert!(steps.contains(&Step::Removed {
        branch: "feat/same".into()
    }));
}

#[test]
fn a_branch_never_pushed_with_work_the_base_lacks_is_kept() {
    let dir = repo();
    git(&dir, &["checkout", "-q", "-b", "feat/lonely", "main"]);
    commit(&dir, "l.txt", "l\n", "unpushed work");
    git(&dir, &["checkout", "-q", "main"]);

    clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(has_local(&dir, "feat/lonely"));
}

#[test]
fn a_branch_behind_the_base_that_adds_nothing_to_it_is_removed_with_its_worktree() {
    let dir = repo();
    let wt = dir.join(".worktrees/feat/behind");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/behind",
            wt.to_str().unwrap(),
            "main",
        ],
    );
    // net-empty branch: an own commit that changes nothing against the base
    git(&wt, &["commit", "-q", "--allow-empty", "-m", "nothing"]);
    // the base moves on, so the branch tree differs from the base tree
    commit(&dir, "later.txt", "later\n", "base moves on");
    git(&dir, &["push", "-q", "origin", "main"]);

    let steps = clean(&ProcessRunner, &dir, "main", true).unwrap();

    assert!(!wt.exists(), "worktree still there");
    assert!(!has_local(&dir, "feat/behind"));
    assert!(steps.contains(&Step::Removed {
        branch: "feat/behind".into()
    }));
}
