//! `branches::catch_up` on real repositories: one case each, always with a
//! copy of the original before anything is rewritten.

use actautoma::branches::{catch_up, CatchUp};
use actautoma::process::ProcessRunner;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// `work` on main with `a.txt` pushed to a bare `origin`, plus a second
/// clone `peer` that stands for the other session.
struct Pair {
    work: PathBuf,
    peer: PathBuf,
}

fn pair() -> Pair {
    let root = std::env::temp_dir().join(format!(
        "actautoma-catchup-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let origin = root.join("origin.git");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    // the replay commits, so the repository carries its own identity
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    commit(&work, "a.txt", "one\ntwo\nthree\n", "base");
    git(&work, &["push", "-q", "origin", "main"]);
    git(&root, &["clone", "-q", origin.to_str().unwrap(), "peer"]);
    Pair {
        work,
        peer: root.join("peer"),
    }
}

fn peer_ships(pair: &Pair, file: &str, text: &str) {
    commit(&pair.peer, file, text, "peer work");
    git(&pair.peer, &["push", "-q", "origin", "main"]);
}

#[test]
fn nothing_behind_means_nothing_to_catch_up() {
    let p = pair();
    commit(&p.work, "mine.txt", "m\n", "mine");
    let outcome = catch_up(&ProcessRunner, &p.work, "main").unwrap();
    assert_eq!(outcome, CatchUp::UpToDate { ahead: 1 });
}

#[test]
fn only_behind_fast_forwards() {
    let p = pair();
    peer_ships(&p, "p.txt", "p\n");
    let outcome = catch_up(&ProcessRunner, &p.work, "main").unwrap();
    assert_eq!(outcome, CatchUp::FastForwarded { commits: 1 });
    assert!(p.work.join("p.txt").exists());
}

#[test]
fn a_diverged_branch_is_replayed_on_the_remote_after_a_copy() {
    let p = pair();
    commit(&p.work, "mine.txt", "m\n", "mine");
    let original = git(&p.work, &["rev-parse", "HEAD"]);
    peer_ships(&p, "p.txt", "p\n");

    let outcome = catch_up(&ProcessRunner, &p.work, "main").unwrap();

    let backup = format!("backup/main/{original}");
    assert_eq!(
        outcome,
        CatchUp::Replayed {
            commits: 1,
            backup: backup.clone()
        }
    );
    assert_eq!(git(&p.work, &["rev-parse", &backup]), original);
    assert!(p.work.join("p.txt").exists() && p.work.join("mine.txt").exists());
    assert_eq!(
        git(&p.work, &["rev-parse", "HEAD~1"]),
        git(&p.work, &["rev-parse", "origin/main"])
    );
}

#[test]
fn a_conflict_names_the_files_and_leaves_the_branch_untouched() {
    let p = pair();
    commit(&p.work, "a.txt", "one\nMINE\nthree\n", "mine");
    let original = git(&p.work, &["rev-parse", "HEAD"]);
    peer_ships(&p, "a.txt", "one\nPEER\nthree\n");

    let outcome = catch_up(&ProcessRunner, &p.work, "main").unwrap();

    assert_eq!(
        outcome,
        CatchUp::Conflict {
            files: vec!["a.txt".to_string()],
            backup: format!("backup/main/{original}"),
        }
    );
    assert_eq!(git(&p.work, &["rev-parse", "HEAD"]), original);
    assert_eq!(git(&p.work, &["status", "--porcelain"]), "");
}

#[test]
fn a_dirty_tree_is_refused_and_named() {
    let p = pair();
    std::fs::write(p.work.join("a.txt"), "edited\n").unwrap();
    let outcome = catch_up(&ProcessRunner, &p.work, "main").unwrap();
    assert_eq!(
        outcome,
        CatchUp::Dirty {
            files: vec!["a.txt".to_string()]
        }
    );
}
